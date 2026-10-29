// Invisible standalone player conversations QA. OpenRouter is intercepted locally;
// the fixture key is never sent to a real service or written to the manifest.
const { chromium } = require(process.env.TTX_PLAYWRIGHT || 'playwright');
const fs = require('fs');
const path = require('path');
const assert = require('assert/strict');

/** The launcher shown before the exercise: resume the saved session, else start the selected scenario. */
async function launch(page) {
  await page.locator('.launcher, [data-panel="master"] #turn-controls').first().waitFor({ state: 'attached', timeout: 60000 });
  const button = page.locator('.launcher #resume-session, .launcher #start-session').first();
  if (await button.count()) await button.click();
}

/** The start-of-session language gate; a no-op when this session already answered it. */
async function pickLanguage(scope, lang) {
  const choice = scope.locator(`#language-gate [data-lang="${lang}"]`);
  if (await choice.count()) await choice.click();
}

const cwd = path.resolve(__dirname, '../..');
const out = path.resolve(process.env.TTX_QA_OUT || path.join(cwd, 'runs/player-conversations-review'));
const base = process.env.TTX_QA_URL || 'http://127.0.0.1:8080';
const exceptionMessage = "Using exceptions for control flow, don't mind me. This isn't actually an error!";

(async () => {
  fs.mkdirSync(out, { recursive: true });
  const browser = await chromium.launch({ headless: true, channel: 'chrome' });
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 }, deviceScaleFactor: 1 });
  const errors = [], runtimeSignals = [], modelTasks = [], promptEvidence = [];
  let expectedReplyReference = null;
  page.on('pageerror', error => {
    if (error.message === exceptionMessage) runtimeSignals.push('winit event-loop control flow');
    else errors.push(error.message);
  });
  await page.route('https://openrouter.ai/api/v1/chat/completions', async route => {
    const request = route.request().postDataJSON();
    const system = request.messages?.[0]?.content || '';
    const task = system.includes('private end-of-turn recap') ? 'player_turn_summary'
      : /translat/i.test(system) ? 'translate' : 'other';
    modelTasks.push(task);
    if (task === 'player_turn_summary') {
      const context = request.messages.map(message => message.content || '').join('\n');
      // Capture only booleans; never persist raw prompts or credentials.
      promptEvidence.push({
        includesReplyText: context.includes('Confermo la ricezione, grazie.'),
        includesReplyReference: expectedReplyReference !== null && context.includes(expectedReplyReference),
      });
    }
    const answer = task === 'player_turn_summary'
      ? 'Turno 1: conversazione aggiornata e risposta collegata.'
      : request.messages?.at(-1)?.content || '';
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      headers: { 'Access-Control-Allow-Origin': '*' },
      body: JSON.stringify({ choices: [{ message: { content: answer } }] }),
    });
  });

  const master = page.locator('[data-panel="master"]');
  const mayor = page.locator('[data-panel="mayor"]');
  const fire = page.locator('[data-panel="fire_commander"]');
  const choose = async role => page.locator(`.role-tabs [data-role="${role}"]`).click();
  const waitPaint = () => page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));

  try {
    await page.goto(base, { waitUntil: 'domcontentloaded' }); await launch(page);
    await master.locator('#start').waitFor({ timeout: 30000 });
    await page.locator('.standalone-settings summary').click();
    await page.locator('#openrouter-key').fill('qa-fixture-only');
    await page.locator('#configure-ai').click();
    assert.equal(await page.locator('#openrouter-key').inputValue(), '', 'fixture credential clears after setup');
    await page.locator('.standalone-settings summary').click();
    await page.getByLabel('Simulazione', { exact: true }).uncheck();
    await require('./standalone-languages.cjs').confirmLanguages(page, { mayor: 'it', fire_commander: 'en' });
    await master.locator('#start').click();
    await master.locator('#turn').filter({ hasText: 'orders' }).waitFor();

    // Deliver a genuine private EXCON inject to the mayor.
    await master.locator('[data-view="timeline"]').click();
    await master.locator('[data-inject-role="mayor"]').click();
    let modal = page.getByRole('dialog');
    await page.waitForFunction(() => document.activeElement?.id === 'inject-title');
    await modal.locator('#inject-title').fill('Municipal coordination');
    await modal.locator('#text').fill('Coordinate a warning for F4 with the fire commander.');
    await modal.locator('#send').click();
    await modal.waitFor({ state: 'hidden' });
    await master.locator('#excon-timeline').getByText('Municipal coordination').waitFor();

    await choose('mayor');
    await pickLanguage(mayor, 'it');
    await mayor.locator('[data-view="map"]').click();
    await mayor.locator('#player-timeline').getByText('Municipal coordination').waitFor();
    const bell = mayor.locator('[data-notification-bell]');
    await bell.click();
    const notificationPanel = mayor.locator('[data-notification-panel]');
    await notificationPanel.waitFor();
    const baselineRead = notificationPanel.locator('[data-mark-notifications-read]');
    if (await baselineRead.isEnabled()) await baselineRead.click();
    await bell.locator('[data-notification-count]').waitFor({ state: 'detached' });
    await mayor.locator('.notification-backdrop').click();
    await notificationPanel.waitFor({ state: 'hidden' });
    await mayor.locator('[data-view="function"]').click();
    await mayor.locator('#function-view').waitFor();
    await page.screenshot({ path: path.join(out, 'mayor-function-desktop.png'), fullPage: true });

    // A peer message creates its own notification and a per-conversation unread count.
    await choose('fire_commander');
    await pickLanguage(fire, 'en');
    await fire.locator('[data-view="chat"]').click();
    await fire.locator('[data-contact="mayor"]').click();
    await fire.locator('#text').fill('I confirm receipt, thank you.');
    await fire.locator('#send').click();
    const fireMessage = fire.locator('#messages [data-chat-message]').filter({ hasText: 'I confirm receipt, thank you.' }).last();
    await fireMessage.waitFor();
    const fireMessageId = await fireMessage.getAttribute('data-chat-message');
    assert(fireMessageId, 'sent message has a stable conversation id');

    await choose('mayor');
    const mayorChatNav = mayor.locator('[data-view="chat"]');
    const unread = mayorChatNav.locator('[data-chat-unread]');
    await unread.filter({ hasText: '1' }).waitFor();
    await bell.locator('[data-notification-count]').filter({ hasText: '1' }).waitFor();
    assert((await bell.getAttribute('aria-label')).includes('non lette'), 'bell label announces unread count');
    const toast = mayor.locator(`[data-notification-toast="message:${fireMessageId}"]`);
    await toast.waitFor();
    assert((await toast.innerText()).length > 0, 'incoming chat displays a toast');
    await page.screenshot({ path: path.join(out, 'mayor-unread-toast-desktop.png'), fullPage: true });
    await bell.click();
    await notificationPanel.waitFor();
    assert.equal(await notificationPanel.locator('[data-open-notification].unread-event').count(), 1,
      'notification dropdown has one unread chat event, separate from read history');
    await page.screenshot({ path: path.join(out, 'mayor-notifications-desktop.png'), fullPage: true });
    await mayor.locator('.notification-backdrop').click();
    await notificationPanel.waitFor({ state: 'hidden' });

    // Opening Conversations marks only that message as read and shows conversation messages.
    await mayorChatNav.click();
    await mayor.locator('[data-contact="fire_commander"]').click();
    await unread.waitFor({ state: 'detached' });
    const mayorFireMessage = mayor.locator(`#messages [data-chat-message="${fireMessageId}"]`);
    await mayorFireMessage.waitFor();
    assert.equal(await mayor.locator('#messages').getByText('Municipal coordination').count(), 0,
      'the EXCON inject stays in the Injects timeline, outside Conversations');
    assert((await mayorFireMessage.innerText()).includes('I confirm receipt, thank you.'));
    await mayorFireMessage.locator(`[data-reply-message="${fireMessageId}"]`).click();
    await mayor.locator(`[data-reply-preview="${fireMessageId}"]`).waitFor();
    await page.screenshot({ path: path.join(out, 'mayor-reply-quote-desktop.png'), fullPage: true });
    await mayor.locator('#text').fill('Confermo la ricezione, grazie.');
    await mayor.locator('#send').click();

    // The reply retains a reference to the original message at both ends.
    await choose('fire_commander');
    const fireMayorReply = fire.locator(`#messages [data-chat-message]`).filter({ hasText: 'Confermo la ricezione, grazie.' }).last();
    await fireMayorReply.waitFor();
    const replyId = await fireMayorReply.getAttribute('data-chat-message');
    await fireMayorReply.locator(`[data-reply-target="${fireMessageId}"]`).waitFor();
    await fireMayorReply.locator(`[data-reply-target="${fireMessageId}"]`).click();
    await fire.locator(`#chat-message-fire_commander-${fireMessageId}`).waitFor();
    await page.screenshot({ path: path.join(out, 'fire-reply-quote-desktop.png'), fullPage: true });
    expectedReplyReference = fireMessageId;
    assert(modelTasks.every(task => task === 'translate'), 'peer chat only uses translation requests before resolution');

    // Profanity is rejected on the sender only, retaining the editable draft.
    await choose('mayor');
    await mayor.locator('[data-view="chat"]').click();
    await mayor.locator('[data-contact="fire_commander"]').click();
    await mayor.locator('#text').fill('Sei uno stronzo.');
    await mayor.locator('#send').click();
    await mayor.locator('#error').filter({ hasText: /insulti|volgar|rifiutat/i }).waitFor();
    assert.equal(await mayor.locator('#text').inputValue(), 'Sei uno stronzo.', 'rejected draft remains editable');
    await choose('fire_commander');
    assert.equal(await fire.locator('#messages').getByText('Sei uno stronzo.', { exact: true }).count(), 0,
      'rejected profanity is not delivered to the recipient');

    // Resolve a turn and make sure the private summary context preserves reply linkage.
    await choose('master');
    await master.locator('[data-view="timeline"]').click();
    await master.locator('#minutes').fill('1');
    await master.locator('#resolve').click();
    await master.locator('#turn').filter({ hasText: 'Turno 2 · orders' }).waitFor({ timeout: 30000 });
    await choose('mayor');
    await mayor.locator('[data-view="chat"]').click();
    const summaryDeadline = Date.now() + 30000;
    while (!promptEvidence.length && Date.now() < summaryDeadline) await page.waitForTimeout(100);
    assert(modelTasks.includes('player_turn_summary'), 'resolution requested the player summary');
    assert(promptEvidence.some(evidence => evidence.includesReplyText), 'summary prompt contains the sent reply context');
    assert(promptEvidence.some(evidence => evidence.includesReplyReference), 'summary prompt retains the replied-to message id');
    assert(await mayor.locator('.chat-turn-separator[data-turn="1"]').count() > 0, 'conversation marks the completed turn');
    await mayor.locator('[data-contact="fire_commander"]').click();
    await mayor.locator('#text').fill('Nuovo aggiornamento dopo il cambio turno.');
    await mayor.locator('#send').click();
    await mayor.locator('.chat-turn-separator[data-turn="2"]').waitFor();
    await mayor.locator('#messages').getByText('Nuovo aggiornamento dopo il cambio turno.', { exact: true }).waitFor();
    await page.screenshot({ path: path.join(out, 'mayor-turn-separated-chat.png'), fullPage: true });

    // A fresh inbound peer message leaves one unread event for the bell's mark-all action.
    await mayor.locator('[data-view="function"]').click();
    await choose('fire_commander');
    await fire.locator('[data-view="chat"]').click();
    await fire.locator('[data-contact="mayor"]').click();
    await fire.locator('#text').fill('Secondo aggiornamento in arrivo.');
    await fire.locator('#send').click();
    await choose('mayor');
    await mayor.locator('[data-view="function"]').click();
    await mayorChatNav.locator('[data-chat-unread]').filter({ hasText: '1' }).waitFor();
    await bell.locator('[data-notification-count]').filter({ hasText: '1' }).waitFor();
    await bell.click();
    await notificationPanel.waitFor();
    await notificationPanel.locator('[data-open-notification].unread-event').filter({ hasText: 'Secondo aggiornamento in arrivo.' }).waitFor();
    await notificationPanel.locator('[data-mark-notifications-read]').click();
    await bell.locator('[data-notification-count]').waitFor({ state: 'detached' });
    await mayorChatNav.locator('[data-chat-unread]').waitFor({ state: 'detached' });
    await mayor.locator('.notification-backdrop').click();
    await notificationPanel.waitFor({ state: 'hidden' });

    await page.setViewportSize({ width: 390, height: 844 });
    await choose('mayor');
    await mayor.locator('[data-view="chat"]').click();
    await mayor.locator('[data-contact="fire_commander"]').click();
    await waitPaint();
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false,
      '390px mobile viewport has no horizontal overflow');
    await page.screenshot({ path: path.join(out, 'mayor-chat-mobile.png'), fullPage: true });

    const lastIncoming = mayor.locator('#messages [data-chat-message]').filter({ hasText: 'Secondo aggiornamento in arrivo.' }).last();
    const lastIncomingId = await lastIncoming.getAttribute('data-chat-message');
    await lastIncoming.locator(`[data-reply-message="${lastIncomingId}"]`).click();
    await mayor.locator(`[data-reply-preview="${lastIncomingId}"]`).waitFor();
    const mobileComposer = await mayor.locator('#composer').boundingBox();
    const mobileTextarea = await mayor.locator('#text').boundingBox();
    const mobileSend = await mayor.locator('#send').boundingBox();
    assert(mobileComposer && mobileTextarea && mobileSend, 'mobile reply composer, textarea, and Send are rendered');
    assert(mobileTextarea.y >= 0 && mobileTextarea.y + mobileTextarea.height <= 844, 'mobile reply textarea fits the viewport');
    assert(mobileSend.y >= 0 && mobileSend.y + mobileSend.height <= 844, 'mobile Send button fits the viewport');
    await page.screenshot({ path: path.join(out, 'mayor-reply-mobile.png'), fullPage: true });
    await mayor.locator('.chat-reply-cancel').click();
    await mayor.locator(`[data-reply-preview="${lastIncomingId}"]`).waitFor({ state: 'detached' });

    await bell.click();
    await notificationPanel.waitFor();
    await page.screenshot({ path: path.join(out, 'mayor-notifications-mobile.png') });
    await mayor.locator('.notification-backdrop').click({ position: { x: 2, y: 2 } });
    await notificationPanel.waitFor({ state: 'hidden' });

    assert.equal(errors.length, 0, errors.join('\n'));
    fs.writeFileSync(path.join(out, 'manifest.json'), JSON.stringify({
      app: base,
      scenario: 'valverde_factory',
      flow: ['start EXCON', 'private mayor inject', 'send peer conversation message', 'unread chat and notification', 'reply quote and navigate to source', 'reject profanity while retaining draft', 'resolve turn and inspect summary context', 'clear notifications', 'mobile overflow, reply composer, and notification dropdown at 390px'],
      modelTasks,
      replyReferenceRetainedInSummary: promptEvidence.some(evidence => evidence.includesReplyReference),
      runtimeSignals,
      viewports: [[1440, 1000], [390, 844]],
      screenshotFiles: ['mayor-function-desktop.png', 'mayor-unread-toast-desktop.png', 'mayor-notifications-desktop.png', 'mayor-reply-quote-desktop.png', 'fire-reply-quote-desktop.png', 'mayor-turn-separated-chat.png', 'mayor-chat-mobile.png', 'mayor-reply-mobile.png', 'mayor-notifications-mobile.png'],
      errors,
    }, null, 2));
    console.log(`PASS player Conversations QA: ${out}`);
  } catch (error) {
    fs.writeFileSync(path.join(out, 'failure.json'), JSON.stringify({ error: String(error), stack: error.stack, errors, modelTasks }, null, 2));
    try { await page.screenshot({ path: path.join(out, 'failure.png'), fullPage: true }); } catch (_) {}
    throw error;
  } finally {
    await browser.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });

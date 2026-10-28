// Invisible standalone threaded-actions QA. OpenRouter is intercepted locally
// before the fixture key is configured; no external model calls are possible.
// Use one browser page because each standalone page owns an independent exercise.
const { chromium } = require(process.env.TTX_PLAYWRIGHT || '../record/node_modules/playwright');
const fs = require('fs');
const path = require('path');
const assert = require('assert/strict');

/** The start-of-session language gate; a no-op when this session already answered it. */
async function pickLanguage(scope, lang) {
  const choice = scope.locator(`#language-gate [data-lang="${lang}"]`);
  if (await choice.count()) await choice.click();
}

const cwd = path.resolve(__dirname, '../..');
const out = path.resolve(process.env.TTX_QA_OUT || path.join(cwd, 'runs/threaded-actions-review'));
const base = process.env.TTX_QA_URL || 'http://127.0.0.1:8080';
const unsupportedText = 'Please acknowledge the situation';
const supportedText = 'Issue a warning for the selected zone';

(async () => {
  fs.mkdirSync(out, { recursive: true });
  const errors = [], modelTasks = [];
  const browser = await chromium.launch({ headless: true, channel: 'chrome' });
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 }, deviceScaleFactor: 1 });
  page.on('pageerror', error => errors.push(error.message));
  await page.route('https://openrouter.ai/api/v1/chat/completions', async route => {
    const request = route.request().postDataJSON();
    const system = request.messages?.[0]?.content || '';
    const preparing = system.includes('BEFORE sending');
    const user = request.messages?.at(-1)?.content || '';
    modelTasks.push(preparing ? 'prepare_decision' : 'translate');
    let content;
    if (preparing) {
      const supported = user.includes(supportedText);
      content = JSON.stringify({
        message: supported ? supportedText : unsupportedText,
        detected_language: 'en',
        translation_en: supported ? supportedText : unsupportedText,
        summary: supported ? 'Issue a warning for the selected zone' : 'Acknowledgment only',
        actions: supported ? [{ type: 'IssueWarning', zone: 'F4' }] : [],
        clarification: null,
        messages: [],
      });
    } else {
      content = user;
    }
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      headers: { 'Access-Control-Allow-Origin': '*' },
      body: JSON.stringify({ choices: [{ message: { content } }] }),
    });
  });

  const panel = role => page.locator(`[data-panel="${role}"]`);
  const choose = async role => page.locator(`.role-tabs [data-role="${role}"]`).click();
  const waitPaint = () => page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  const clickMap = async (map, xFraction, yFraction) => {
    const box = await map.boundingBox();
    assert(box && box.width > 0 && box.height > 0, 'work map canvas is visible');
    await map.click({ position: { x: Math.max(2, box.width * xFraction), y: Math.max(2, box.height * yFraction) } });
  };

  try {
    await page.goto(base, { waitUntil: 'domcontentloaded' });
    const master = panel('master');
    await master.locator('#start').waitFor({ timeout: 30000 });

    // Configure only the mock-backed fixture credential and pause optional 3D
    // simulation. Every actor stays in this same standalone browser page.
    await page.locator('.standalone-settings summary').click();
    await page.locator('#openrouter-key').fill('qa-fixture-only');
    await page.locator('#configure-ai').click();
    assert.equal(await page.locator('#openrouter-key').inputValue(), '', 'fixture credential clears after setup');
    await page.locator('.standalone-settings summary').click();
    await page.getByLabel('Simulazione', { exact: true }).uncheck();
    await master.locator('#start').click();
    await master.locator('#turn').filter({ hasText: 'orders' }).waitFor();

    // EXCON delivers two private injects to the mayor using the real composer.
    await master.locator('[data-view="timeline"]').click();
    const sendInject = async (title, text) => {
      await master.locator('[data-inject-role="mayor"]').click();
      const dialog = page.getByRole('dialog');
      await page.waitForFunction(() => document.activeElement?.id === 'inject-title');
      await dialog.locator('#inject-title').fill(title);
      await dialog.locator('#inject-sender').fill('EXCON QA');
      await dialog.locator('#text').fill(text);
      await dialog.locator('#send').click();
      await dialog.waitFor({ state: 'hidden' });
      await master.locator('#excon-timeline').getByText(title).waitFor();
    };
    await sendInject('First coordination request', 'Coordinate with the fire commander.');
    await sendInject('Latest coordination request', 'Coordinate the response for the latest report.');

    const mayor = panel('mayor');
    const fire = panel('fire_commander');
    await choose('mayor');
    await pickLanguage(mayor, 'en');
    await mayor.locator('[data-view="chat"]').click();
    const injectReference = mayor.locator('#chat-inject-reference-mayor');
    await injectReference.waitFor();
    const injectOptions = await injectReference.locator('option').evaluateAll(options => options.map(option => ({
      value: option.value,
      label: option.textContent,
    })));
    assert(injectOptions.length >= 2, 'both injects can be referenced from the composer');
    const latestInjectId = injectOptions[0].value;
    const earlierInjectId = injectOptions[1].value;
    assert.equal(await injectReference.inputValue(), latestInjectId, 'latest received inject is preselected');
    await mayor.locator('[data-contact="fire_commander"]').click();
    await injectReference.selectOption(earlierInjectId);
    await mayor.locator('#text').fill('Mayor update: coordinate the response with the fire commander.');
    await mayor.locator('#send').click();
    const mayorMessage = mayor.locator('#messages [data-chat-message]').filter({ hasText: 'Mayor update: coordinate the response' }).last();
    await mayorMessage.waitFor();
    const mayorMessageId = await mayorMessage.getAttribute('data-chat-message');
    assert(mayorMessageId, 'mayor chat message has a stable id');

    // Fire replies to the mayor message. The mayor then proposes an action from
    // that received peer bubble, using the same map-backed work modal.
    await choose('fire_commander');
    await pickLanguage(fire, 'en');
    await fire.locator('[data-view="chat"]').click();
    await fire.locator('[data-contact="mayor"]').click();
    const fireIncoming = fire.locator(`#messages [data-chat-message="${mayorMessageId}"]`);
    await fireIncoming.waitFor();
    await fireIncoming.locator(`[data-reply-message="${mayorMessageId}"]`).click();
    await fire.locator('#text').fill('Fire commander reply: we are ready to coordinate.');
    await fire.locator('#send').click();
    const fireReply = fire.locator('#messages [data-chat-message]').filter({ hasText: 'Fire commander reply: we are ready' }).last();
    await fireReply.waitFor();
    const fireMessageId = await fireReply.getAttribute('data-chat-message');

    await choose('mayor');
    await mayor.locator('[data-view="chat"]').click();
    await mayor.locator('[data-contact="fire_commander"]').click();
    const mayorIncoming = mayor.locator(`#messages [data-chat-message="${fireMessageId}"]`);
    await mayorIncoming.waitFor();
    await mayorIncoming.locator(`[data-propose-message="${fireMessageId}"]`).click();
    const work = mayor.locator('.work-modal');
    await work.waitFor();
    assert.equal(await work.locator('#work-reply-to').inputValue(), fireMessageId,
      'proposal opens with the selected peer chat as its parent');
    await work.locator('#work-action').selectOption('IssueWarning');
    await clickMap(work.locator('canvas#work-map'), 0.5, 0.45);
    assert((await work.locator('#work-location').inputValue()).trim(), 'map click selects a zone for the action');
    await page.screenshot({ path: path.join(out, 'threaded-work-modal-desktop.png') });
    await work.locator('[data-add-action]').click();
    await work.locator('[data-respond]').click();
    await work.waitFor({ state: 'hidden' });

    // EXCON reviews the action and the messages that form its thread.
    await choose('master');
    const exconThread = master.locator('#excon-timeline .proposal-thread');
    await exconThread.first().waitFor();
    await exconThread.locator(`[data-thread-message="${fireMessageId}"]`).waitFor();
    await page.screenshot({ path: path.join(out, 'excon-proposal-thread-desktop.png'), fullPage: true });

    // The earlier inject can receive multiple action responses.
    await choose('mayor');
    await mayor.locator('[data-view="map"]').click();
    for (let response = 0; response < 2; response++) {
      await mayor.locator('#new-player-action').click();
      await work.waitFor();
      await work.locator('#work-reply-to').selectOption(earlierInjectId);
      await work.locator('#work-action').selectOption('IssueWarning');
      await work.locator('#work-location').fill('F4');
      await work.locator('[data-add-action]').click();
      await work.locator('[data-respond]').click();
      await work.waitFor({ state: 'hidden' });
    }
    await choose('master');
    const priorProposals = master.locator('.proposal-inline[data-proposal]');
    await page.waitForFunction(() => document.querySelectorAll('[data-panel="master"] .proposal-inline[data-proposal]').length >= 3);
    const proposalCountBeforeAi = await priorProposals.count();

    // Unsupported mock result is explicitly blocked and remains private.
    await choose('mayor');
    await mayor.locator('[data-view="map"]').click();
    await mayor.locator('#new-player-action').click();
    await work.waitFor();
    await work.locator('.work-ai summary').click();
    await work.locator('#work-free-text').fill(unsupportedText);
    await work.getByRole('button', { name: /prepare AI draft|prepara bozza AI/i }).click();
    await work.getByText(/no verifiable operational actions|non contiene azioni operative verificabili/i).waitFor();
    const confirm = work.locator('[data-stage-draft]');
    assert(await confirm.isDisabled(), 'unsupported AI draft cannot be added to the response');
    assert.equal(await master.locator('.proposal-inline[data-proposal]').count(), proposalCountBeforeAi,
      'unsupported AI draft creates no EXCON proposal while its modal remains open');

    // A valid coded action becomes reviewable but stays private until explicit
    // confirmation. The gateway returns an action only for this exact request.
    await work.locator('#work-free-text').fill(supportedText);
    await work.getByRole('button', { name: /prepare AI draft|prepara bozza AI/i }).click();
    await work.getByText(supportedText).waitFor();
    assert(!(await confirm.isDisabled()), 'coded AI action enables the explicit confirmation');
    assert.equal(await master.locator('.proposal-inline[data-proposal]').count(), proposalCountBeforeAi,
      'AI interpretation is not sent before confirmation while its modal remains open');
    await confirm.click();
    await work.locator('[data-staged-action]').waitFor();
    await work.locator('[data-respond]').click();
    await work.waitFor({ state: 'hidden' });
    await choose('master');
    await page.waitForFunction(count => document.querySelectorAll('[data-panel="master"] .proposal-inline[data-proposal]').length > count, proposalCountBeforeAi);
    assert(modelTasks.includes('prepare_decision'), 'AI drafts used the intercepted mock gateway');
    await page.screenshot({ path: path.join(out, 'excon-confirmed-ai-action.png'), fullPage: true });

    // Fire commander can place a line using two separate map clicks. Verify the
    // mobile modal geometry and endpoint fields without sending the proposal.
    await page.setViewportSize({ width: 390, height: 844 });
    await choose('fire_commander');
    await fire.locator('[data-view="chat"]').click();
    await fire.locator('[data-contact="mayor"]').click();
    const originalMayorMessage = fire.locator(`#messages [data-chat-message="${mayorMessageId}"]`);
    await originalMayorMessage.locator(`[data-propose-message="${mayorMessageId}"]`).click();
    const fireWork = fire.locator('.work-modal');
    await fireWork.waitFor();
    await fireWork.locator('#work-action').selectOption('CutFireLine');
    const firstPoint = fireWork.getByRole('button', { name: /first point|primo punto/i });
    const secondPoint = fireWork.getByRole('button', { name: /second point|secondo punto/i });
    await firstPoint.waitFor();
    await firstPoint.click();
    await clickMap(fireWork.locator('canvas#work-map'), 0.32, 0.38);
    const firstValue = await fireWork.locator('#work-location').inputValue();
    await secondPoint.click();
    await clickMap(fireWork.locator('canvas#work-map'), 0.72, 0.66);
    const secondValue = await fireWork.locator('#work-destination').inputValue();
    assert(firstValue && secondValue && firstValue !== secondValue, 'CutFireLine endpoints can be chosen independently');
    await waitPaint();
    await page.screenshot({ path: path.join(out, 'threaded-work-modal-mobile.png'), fullPage: true });

    assert.equal(errors.length, 0, errors.join('\n'));
    fs.writeFileSync(path.join(out, 'manifest.json'), JSON.stringify({
      fixture: 'single standalone exercise; local fake OpenRouter gateway; no external model calls',
      flow: ['EXCON sends two mayor injects', 'latest inject preselected in peer composer', 'mayor selects earlier inject reference', 'fire commander replies to mayor chat', 'map-backed action proposal from reply bubble', 'EXCON inspects proposal thread', 'two action responses reference the same inject', 'unsupported AI draft blocked', 'coded AI action waits for explicit confirmation', 'mobile CutFireLine endpoint selection'],
      injectIds: [earlierInjectId, latestInjectId],
      chatIds: [mayorMessageId, fireMessageId],
      modelTasks,
      viewports: [[1440, 1000], [390, 844]],
      screenshots: ['threaded-work-modal-desktop.png', 'excon-proposal-thread-desktop.png', 'excon-confirmed-ai-action.png', 'threaded-work-modal-mobile.png'],
      errors,
    }, null, 2));
    console.log(`PASS threaded actions QA: ${out}`);
  } catch (error) {
    fs.writeFileSync(path.join(out, 'failure.json'), JSON.stringify({ error: String(error), stack: error.stack, errors, modelTasks }, null, 2));
    try { await page.screenshot({ path: path.join(out, 'failure.png'), fullPage: true }); } catch (_) {}
    throw error;
  } finally {
    await browser.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });

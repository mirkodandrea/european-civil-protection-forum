// Invisible standalone QA of the "Risposte" flow (playtest 28/10): one inject,
// a conversation started from it, responses with an action chosen on the map,
// several responses to the same inject, AI suggestions in a single action slot,
// a plain communication and the fire-line endpoints on a narrow screen.
// OpenRouter is intercepted locally before the fixture key is configured; no
// external model calls are possible. One page: each standalone page owns its exercise.
const { chromium } = require(process.env.TTX_PLAYWRIGHT || 'playwright');
const fs = require('fs');
const path = require('path');
const assert = require('assert/strict');

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
  page.on('pageerror', error => { if (!error.message.includes('control flow')) errors.push(error.message); });
  await page.route('https://openrouter.ai/api/v1/chat/completions', async route => {
    const request = route.request().postDataJSON();
    const system = request.messages?.[0]?.content || '';
    const suggesting = system.includes('ONLY what this role knows');
    const user = request.messages?.at(-1)?.content || '';
    modelTasks.push(suggesting ? 'suggest_actions' : 'translate');
    const supported = suggesting && user.includes(supportedText);
    const content = suggesting
      ? JSON.stringify({ actions: supported ? [{ type: 'IssueWarning', zone: 'F4' }, { type: 'IssueWarning', zone: 'G4' }] : [],
          reasons: supported ? ['The narrative warns the zone', 'Neighbouring zone'] : [], summary: supported ? supportedText : 'Acknowledgment only' })
      : user;
    await route.fulfill({ status: 200, contentType: 'application/json', headers: { 'Access-Control-Allow-Origin': '*' },
      body: JSON.stringify({ choices: [{ message: { content } }] }) });
  });

  const panel = role => page.locator(`[data-panel="${role}"]`);
  const choose = role => page.locator(`.role-tabs [data-role="${role}"]`).click();
  const pickLanguage = async role => { const c = panel(role).locator('#language-gate [data-lang="en"]'); if (await c.count()) await c.click(); };
  const waitPaint = () => page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  const clickMap = async (map, xFraction, yFraction) => {
    const box = await map.boundingBox();
    assert(box && box.width > 0 && box.height > 0, 'work map canvas is visible');
    await map.click({ position: { x: Math.max(2, box.width * xFraction), y: Math.max(2, box.height * yFraction) } });
  };
  const master = panel('master'), mayor = panel('mayor'), fire = panel('fire_commander');
  const work = mayor.locator('.work-modal');
  const openInject = async (role, n = 0) => {
    await choose(role); await pickLanguage(role);
    await panel(role).locator('[data-view="map"]').click();
    const back = panel(role).locator('[data-inject-back]');
    if (await back.count()) await back.click();
    await panel(role).locator('[data-inject-row]').nth(n).click();
  };
  const pending = async () => {
    await choose('master'); await master.locator('[data-view="risposte"]').click();
    const badge = master.locator('.pending-count');
    return (await badge.isVisible()) ? parseInt(await badge.innerText(), 10) : 0;
  };

  try {
    await page.goto(base, { waitUntil: 'domcontentloaded' });
    await page.evaluate(() => { localStorage.clear(); sessionStorage.clear(); });
    await page.goto(base, { waitUntil: 'domcontentloaded' });
    await page.locator('.launcher').waitFor({ timeout: 90000 });
    // The mock-backed fixture credential is set in the launcher, before the exercise.
    await page.locator('#launch-openrouter-key').fill('qa-fixture-only');
    await page.locator('#launch-save-key').click();
    await page.locator('.launcher #start-session').click();
    await master.locator('#start').waitFor({ timeout: 60000 });
    const sim = page.getByLabel('Simulazione', { exact: true }); if (await sim.isChecked()) await sim.uncheck();
    await require('./standalone-languages.cjs').confirmLanguages(page, { mayor: 'en', fire_commander: 'en' });
    await master.locator('#start').click();
    await master.locator('#turn').filter({ hasText: /orders|ordini/ }).waitFor();

    // EXCON delivers the first planned inject to all its recipients.
    await master.locator('[data-view="timeline"]').click();
    await master.locator('[data-send-open]').first().click();
    await master.locator('[data-send]').click();
    await page.waitForTimeout(800);

    // The mayor starts a conversation from the inject; the reference is fixed at opening.
    await openInject('mayor');
    assert.equal(await mayor.locator('#new-player-action').count(), 0, 'no "+ New action" entry point');
    await mayor.locator('[data-reply-conversation]').click();
    await mayor.locator('[data-conv-person="fire_commander"]').check();
    await mayor.locator('#conv-first').fill('Mayor: can you hold the north road while we warn F4?');
    await mayor.locator('[data-create-conversation]').click();
    await mayor.locator('[data-conversation] [data-conv-quote]').waitFor();

    // The fire commander answers in chat, citing the inject: an intent, not an action.
    await openInject('fire_commander');
    await fire.locator('[data-conv-row]').first().click();
    const delivery = await fire.locator('[data-cite-target]').getAttribute('data-cite-target');
    await fire.locator('.conv-quote-cite .conv-cite').click();
    await fire.locator(`[data-reply-preview="${delivery}"]`).waitFor();
    await fire.locator('#conv-text').fill('Going to recon the ridge, will report back.');
    await fire.locator('[data-send-conversation]').click();
    await fire.locator('[data-conv-message]').filter({ hasText: 'Going to recon' }).locator(`[data-conv-cites="${delivery}"]`).waitFor();
    assert.equal(await pending(), 0, 'a chat message is never a proposal');

    // From the conversation the mayor answers with a map-chosen action, to the fire commander.
    await openInject('mayor');
    await mayor.locator('[data-conv-row]').first().click();
    await mayor.locator('[data-conv-message]').filter({ hasText: 'Going to recon' }).waitFor();
    await mayor.locator('[data-respond-from-conversation]').click();
    await work.waitFor();
    assert(await work.locator('#to-players').isChecked(), 'from a conversation the recipients are its participants');
    assert(await work.locator('[data-to-player="fire_commander"]').isChecked());
    assert.equal(await work.locator('#work-reply-to').count(), 0, 'no reply-to selector');
    await work.locator('#kind-decision').check();
    await work.locator('#work-action').selectOption('IssueWarning');
    await clickMap(work.locator('canvas#work-map'), 0.5, 0.45);
    assert((await work.locator('#work-location').inputValue()).trim(), 'map click selects a zone for the action');
    await work.locator('[data-add-action]').click();
    await work.locator('[data-staged-action]').waitFor();
    await work.locator('#response-narrative').fill('We warn the zone; fire holds the north road.');
    assert(!(await work.locator('[data-respond]').isEnabled()), 'a decision waits for its basis');
    await work.locator('#basis-plan').check();
    await page.screenshot({ path: path.join(out, 'threaded-work-modal-desktop.png') });
    await work.locator('[data-respond]').click();
    await work.waitFor({ state: 'hidden' });
    await mayor.locator('[data-conv-response-tag]').waitFor();
    assert.equal(await pending(), 1);

    // EXCON reviews it under the mayor's row, with the conversation below the replies.
    await master.locator('[data-response]').filter({ hasText: /Mayor|Sindaco/ }).locator('button').click();
    const dialog = master.locator('.response-dialog');
    await dialog.locator('[data-response-state="rs_review"]').waitFor();
    await dialog.locator('[data-linked-conversation]').waitFor();
    await dialog.locator('[data-proposal]').first().waitFor();
    await page.screenshot({ path: path.join(out, 'excon-proposal-thread-desktop.png'), fullPage: true });
    await dialog.getByRole('button', { name: /^(Chiudi|Close)$/ }).first().click();

    // The same inject can receive more responses (each one is its own proposal).
    for (const zone of ['F4', 'G4']) {
      await openInject('mayor');
      await mayor.locator('[data-reply-direct]').click();
      await work.locator('#kind-decision').check();
      await work.locator('#work-action').selectOption('IssueWarning');
      await work.locator('#work-location').fill(zone);
      await work.locator('[data-add-action]').click();
      await work.locator('#basis-operational_judgment').check();
      await work.locator('[data-respond]').click();
      await work.waitFor({ state: 'hidden' });
    }
    assert.equal(await pending(), 3);

    // AI suggestions: nothing actionable proposes nothing; an actionable narrative
    // offers actions, and associating one (the only slot) is an explicit click.
    await openInject('mayor');
    await mayor.locator('[data-reply-direct]').click();
    await work.locator('#response-narrative').fill(unsupportedText);
    await work.locator('[data-work-suggestions="none"]').waitFor({ timeout: 15000 });
    await work.locator('#response-narrative').fill(supportedText);
    await work.locator('[data-add-suggestion="1"]').waitFor({ timeout: 15000 });
    assert.equal(await work.locator('[data-staged-action]').count(), 0, 'suggestions are not staged by themselves');
    assert.equal(await work.locator('[data-add-suggestions]').count(), 0, 'no "add all": one action per response');
    await work.locator('[data-add-suggestion="0"]').click();
    await work.locator('[data-add-suggestion="0"]').click();
    assert.equal(await work.locator('[data-staged-action]').count(), 1, 'a second action replaces the first');
    assert(await work.locator('#kind-communication').isDisabled(), 'an action makes it a decision');
    await work.locator('#basis-law').check();
    await work.locator('[data-respond]').click();
    await work.waitFor({ state: 'hidden' });
    assert.equal(await pending(), 4);
    assert(modelTasks.includes('suggest_actions'), 'AI suggestions used the intercepted mock gateway');

    // A plain communication is delivered at once and never waits for EXCON.
    await openInject('mayor');
    await mayor.locator('[data-reply-direct]').click();
    await work.locator('#kind-communication').check();
    assert.equal(await work.locator('[data-work-block="basis"]').count(), 0, 'no basis for a communication');
    await work.locator('#response-narrative').fill('Received, following the situation.');
    await work.locator('[data-respond]').click();
    await work.waitFor({ state: 'hidden' });
    await mayor.locator('[data-response-state="rs_sent"]').waitFor();
    assert.equal(await pending(), 4, 'communications need no approval');
    await page.screenshot({ path: path.join(out, 'excon-confirmed-ai-action.png'), fullPage: true });

    // Fire commander places a line with two separate map clicks on a narrow screen,
    // without sending it.
    await page.setViewportSize({ width: 390, height: 844 });
    await openInject('fire_commander');
    await fire.locator('[data-reply-direct]').click();
    const fireWork = fire.locator('.work-modal');
    await fireWork.locator('#kind-decision').check();
    await fireWork.locator('#work-action').selectOption('CutFireLine');
    const firstPoint = fireWork.getByRole('button', { name: /first point|primo punto/i });
    const secondPoint = fireWork.getByRole('button', { name: /second point|secondo punto/i });
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
      flow: ['EXCON sends an inject', 'mayor starts a conversation from it', 'fire commander chats citing the inject (no proposal)',
        'mayor answers from the conversation to the fire commander with a map-chosen action and a basis',
        'EXCON reviews it with the conversation', 'two more responses to the same inject', 'AI suggestions in one action slot',
        'communication without approval', 'narrow-screen CutFireLine endpoint selection'],
      delivery, modelTasks, viewports: [[1440, 1000], [390, 844]],
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

// Standalone QA of the "only the EXCON acts" flow (2026-10-29), the acceptance
// scenario: the DOS replies in words, the AI (a scripted fixture, no external
// model) reads the thread into EXCON suggestions, the EXCON executes the
// complete ones, reviews and sends the question, the DOS answers, the new
// reading completes the fire line (diff shown) and proposes a cross-role
// question; the Mayor replies, the EXCON executes. Screenshots of every step
// go to TTX_QA_OUT (default runs/excon-only-actions).
//   TTX_QA_URL=http://127.0.0.1:8091/ node scripts/qa/excon-only-actions.cjs
const { chromium } = require(process.env.TTX_PLAYWRIGHT || 'playwright');
const assert = require('assert/strict');
const fs = require('fs');
const path = require('path');
const root = path.resolve(__dirname, '../..');
const out = path.resolve(process.env.TTX_QA_OUT || path.join(root, 'runs/excon-only-actions'));
const url = process.env.TTX_QA_URL || ('file://' + path.join(root, process.env.TTX_STANDALONE_FILE || 'web/standalone/ttx.html'));

const INJECT = 'Ore 14:20, il fronte ha scavalcato il crinale in E3 e scende verso F4, vento W 25 km/h';
const DOS = 'Mandiamo due squadre AIB sul lato nord di F4 e apriamo una linea tagliafuoco';
const ANSWER = 'Lungo la strada da E3 a F3, con il bulldozer del Comune';
const MAYOR = 'Evacuiamo F4, apriamo la palestra. Chiudo la provinciale a F4';

/** The scripted reading of a reply thread (`review_reply`). */
function reading(user, gazetteer) {
  const road = (gazetteer.match(/"roads?"\s*:\s*\[\s*\{[^}]*?"id"\s*:\s*"([^"]+)"/) || [])[1] || 'sp1';
  const shelter = (gazetteer.match(/"name"\s*:\s*"([^"]*Sport[^"]*)"/) || [])[1] || 'Sports Centre';
  if (user.includes(MAYOR)) return {
    actions: [
      { type: 'EvacuateZone', role: 'mayor', params: { zone: 'F4' }, quote: 'Evacuiamo F4' },
      { type: 'OpenShelter', role: 'mayor', params: { building: shelter }, quote: 'apriamo la palestra' },
      { type: 'CloseRoad', role: 'mayor', params: { road }, quote: 'Chiudo la provinciale a F4', notes: ['Provincial road: ' + road] },
    ], questions: [], summary: 'Evacuation of F4, shelter, road closure.' };
  const teams = [1, 2].map(n => ({ type: 'DeployEngine', role: 'fire_commander', params: { at: 'F4:N' }, quote: 'Mandiamo due squadre AIB sul lato nord di F4', notes: [`AIB-${n}: ETA about 12 min from the staging base.`] }));
  if (user.includes(ANSWER)) return {
    actions: [...teams, { type: 'CutFireLine', role: 'fire_commander', params: { from: 'E3', to: 'F3' }, quote: 'Lungo la strada da E3 a F3',
      notes: ['The bulldozer belongs to the Municipality: has the Mayor authorised it?'] }],
    questions: [{ to: 'mayor', text: 'Il DOS chiede il bulldozer del Comune per la linea tagliafuoco E3–F3: lo autorizza?', about: [2], reason: 'Cross-role: the DOS uses the Municipality’s bulldozer.' }],
    summary: 'The answer gives the fire line endpoints.' };
  return {
    actions: [...teams, { type: 'CutFireLine', role: 'fire_commander', params: {}, quote: 'apriamo una linea tagliafuoco', notes: ['Endpoints and means not stated.'] }],
    questions: [{ to: 'fire_commander', text: 'Per la linea tagliafuoco: tra quali punti e con quali mezzi?', about: [2] }],
    summary: 'Two AIB teams to the north side of F4; a fire line without endpoints.' };
}

(async () => {
  fs.mkdirSync(out, { recursive: true });
  const errors = [], tasks = [];
  const browser = await chromium.launch({ headless: true, channel: process.env.TTX_BROWSER_CHANNEL || 'chrome' });
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 }, deviceScaleFactor: 1 });
  page.on('pageerror', e => { if (!e.message.includes('control flow')) errors.push(e.message); });
  await page.route('https://openrouter.ai/api/v1/chat/completions', async route => {
    const req = route.request().postDataJSON();
    const system = req.messages?.[0]?.content || '';
    const user = req.messages?.at(-1)?.content || '';
    let content;
    if (system.includes('Players never act on the simulator')) {
      tasks.push('review_reply');
      content = JSON.stringify(reading(user, system));
    } else { tasks.push('other'); content = user.replace(/^.*?:\n/s, m => m.length < 80 ? '' : m); }
    if (!system.includes('Players never act') && system.startsWith('Translate')) content = user;
    await route.fulfill({ status: 200, contentType: 'application/json', headers: { 'Access-Control-Allow-Origin': '*' },
      body: JSON.stringify({ choices: [{ message: { content } }] }) });
  });
  const panel = role => page.locator(`[data-panel="${role}"]`);
  const choose = role => page.locator(`.role-tabs [data-role="${role}"]`).click();
  const master = panel('master'), fire = panel('fire_commander'), mayor = panel('mayor');
  const shot = async name => { await page.waitForTimeout(300); await page.screenshot({ path: path.join(out, name + '.png') }); };
  const board = master.locator('#review-board');
  const toBoard = async () => { await choose('master'); await master.locator('[data-view="risposte"]').click(); await board.waitFor(); };
  const openInject = async (p, role) => {
    await choose(role);
    await p.locator('[data-view="map"]').click();
    const back = p.locator('[data-inject-back]');
    if (await back.count()) await back.click();
    await p.locator('[data-inject-row]').first().click();
  };
  try {
    await page.goto(url, { waitUntil: 'domcontentloaded' });
    if (url.startsWith('http')) { await page.evaluate(() => { localStorage.clear(); sessionStorage.clear(); }); await page.goto(url, { waitUntil: 'domcontentloaded' }); }
    await page.locator('.launcher').waitFor({ timeout: 90000 });
    if (process.env.TTX_QA_SCENARIO) await page.locator(`input[name="launch-scenario"][value="${process.env.TTX_QA_SCENARIO}"]`).check();
    await page.locator('#launch-openrouter-key').fill('qa-fixture-only');
    await page.locator('#launch-save-key').click();
    await page.locator('.launcher #start-session').click();
    await master.locator('#start').waitFor({ timeout: 60000 });
    const sim = page.getByLabel('Simulazione', { exact: true }); if (await sim.count() && await sim.isChecked()) await sim.uncheck();
    await require('./standalone-languages.cjs').confirmLanguages(page, { fire_commander: 'it', mayor: 'it' });
    await master.locator('#start').click();
    await master.locator('#turn').filter({ hasText: /orders|ordini/ }).waitFor();

    // EXCON sends the inject to the DOS and the Mayor.
    await master.locator('[data-view="timeline"]').click();
    await master.locator('[data-inject-all]').click();
    const modal = page.getByRole('dialog');
    await modal.locator('#inject-title').fill('Il fronte scavalca il crinale');
    await modal.locator('#inject-sender').fill('Sala operativa');
    await modal.locator('#text').fill(INJECT);
    await modal.locator('#inject-scope').selectOption('custom');
    await modal.getByLabel(/Comandante|Commander|DOS/).first().check();
    await modal.getByLabel(/Sindaco|Mayor/).check();
    await modal.locator('#send').click();
    await modal.waitFor({ state: 'hidden' });

    // 1. The DOS replies in words, next to the guidance; no action controls.
    await openInject(fire, 'fire_commander');
    await shot('01-player-inject');
    // One composer under the inject (recipient, text, basis, one Send).
    const work = fire.locator('[data-reply-composer]');
    await work.waitFor();
    for (const forbidden of ['#kind-decision', '[data-add-action]', '[data-suggested-action]'])
      assert.equal(await work.locator(forbidden).count(), 0, `no ${forbidden} on the player page`);
    await work.locator('#basis-operational_judgment').waitFor();
    await work.locator('.reply-guide-more summary').click();
    await work.locator('[data-reply-guide]').waitFor();
    await work.locator('#response-narrative').fill(DOS);
    await shot('02-player-reply-guidance');
    await work.locator('[data-respond]').click();
    await fire.locator('[data-response-state="rs_sent"]').first().waitFor();
    await shot('03-player-reply-sent');

    // 2. EXCON: three suggestions, the fire line incomplete, a question to review.
    await toBoard();
    await board.locator('[data-review-state="question_draft"]').waitFor({ timeout: 20000 });
    assert.equal(await board.locator('[data-review-item]').count(), 3);
    assert.equal(await board.locator('[data-question-state="draft"]').count(), 1);
    await shot('04-excon-suggestions');
    assert.equal(await fire.locator('[data-send-clarification]').count(), 0, 'the draft did not reach the DOS');

    // 3. Complete suggestions run at once: open the first, fix the place if
    // the simulator says it is not feasible, execute both.
    // The simulator may refuse the AI's place (hose reach from a road): the
    // EXCON edits it, trying the places a director would, until it runs.
    const places = ['F4', 'Pineta', 'Valverde', 'E5', 'F5', 'E6', 'Valverde Town Hall'];
    for (let k = 0; k < 2; k++) {
      const id = await board.locator('[data-review-item][data-review-state="infeasible"], [data-review-item][data-review-state="complete"]').first().getAttribute('data-review-item');
      const row = board.locator(`[data-review-item="${id}"]`);
      for (const place of places) {
        if (await row.getAttribute('data-review-state') === 'complete') await row.locator('[data-review-execute]').click();
        else {
          if (!(await row.locator('[data-review-field$=":at"]').count())) await row.locator('[data-review-details]').click();
          await row.locator('[data-review-field$=":at"]').fill(place);
          if (k === 0 && place === places[0]) await shot('05-excon-edit-parameters');
          await row.locator('[data-review-execute]').click();
        }
        await page.waitForTimeout(500);
        if (['queued','executed'].includes(await row.getAttribute('data-review-state'))) break;
      }
      assert(['queued','executed'].includes(await row.getAttribute('data-review-state')), 'a team is queued once the place is feasible');
    }
    await shot('06-excon-executed');
    const executed = await board.locator('[data-review-state="executed"],[data-review-state="queued"]').count();

    // 4. EXCON reviews the question and sends it.
    const q = board.locator('[data-question-state="draft"]').first();
    await q.locator('textarea').fill('Per la linea tagliafuoco: tra quali punti e con quali mezzi? Indica le zone (es. E3, F3) come nella guida.');
    await q.locator('[data-review-send]').click();
    await board.locator('[data-question-state="sent"]').waitFor();
    await board.locator('[data-review-state="awaiting_answer"]').waitFor();
    await shot('07-excon-question-sent');

    // 5. The DOS answers in the same thread.
    await openInject(fire, 'fire_commander');
    await fire.locator('[data-send-clarification]').waitFor();
    await shot('08-player-question');
    await fire.locator('.clarify-reply textarea').fill(ANSWER);
    await fire.locator('[data-send-clarification]').click();
    await page.waitForTimeout(600);
    await shot('09-player-answered');

    // 6. New reading: fire line completed (diff), cross-role question proposed.
    await toBoard();
    await board.locator('[data-review-diff]').first().waitFor({ timeout: 20000 });
    await board.locator('[data-question-state="draft"]').filter({ hasText: /Sindaco|Mayor/ }).waitFor();
    assert(await board.locator('[data-review-state="executed"],[data-review-state="queued"]').count() >= executed, 'decisions taken stay');
    const line = board.locator('[data-review-item]').filter({ hasText: /tagliafuoco|fire line|E3/ }).first();
    await line.locator('[data-review-details]').click();
    await shot('10-excon-diff-cross-role');
    if (await line.locator('[data-review-execute]').isEnabled()) await line.locator('[data-review-execute]').click();
    await board.locator('[data-review-discard]').first().click();
    await page.waitForTimeout(400);

    // 7. The Mayor replies; the EXCON executes what is feasible.
    await openInject(mayor, 'mayor');
    await mayor.locator('[data-reply-composer] #response-narrative').fill(MAYOR);
    await mayor.locator('[data-reply-composer] [data-respond]').click();
    await toBoard();
    await board.locator('[data-review]').filter({ hasText: /Evacuiamo/ }).locator('[data-review-item]').nth(2).waitFor({ timeout: 20000 });
    await shot('11-excon-mayor-suggestions');
    const group = board.locator('[data-review]').filter({ hasText: /Evacuiamo/ });
    for (let k = 0; k < 3 && await group.locator('[data-review-execute]:not([disabled])').count(); k++) {
      await group.locator('[data-review-execute]:not([disabled])').first().click(); await page.waitForTimeout(500);
    }
    await shot('12-excon-mayor-executed');

    // Timeline: the turn panel keeps open questions in sight.
    await master.locator('[data-view="timeline"]').click();
    await shot('13-excon-timeline');
    assert(tasks.includes('review_reply'));
    assert.deepEqual(errors, []);
    console.log('excon-only-actions QA ok:', out);
  } catch (e) {
    await page.screenshot({ path: path.join(out, 'failure.png') });
    console.error(e); console.error(errors);
    process.exitCode = 1;
  } finally { await browser.close(); }
})();

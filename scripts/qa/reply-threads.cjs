// Standalone QA of successive replies to one inject (playtest 30/10: "the
// replies get messed up as they keep arriving"). The DOS replies, the EXCON
// handles every suggestion (queue, ignore, discard the question): the reply
// closes by itself, leaves the pending badge and moves to "Handled replies".
// The DOS replies again: a new, distinct group; the old one stays closed.
// The AI is a scripted fixture. Screenshots go to TTX_QA_OUT (default runs/reply-threads).
//   TTX_QA_URL=http://127.0.0.1:8082/ node scripts/qa/reply-threads.cjs
const { chromium } = require(process.env.TTX_PLAYWRIGHT || 'playwright');
const assert = require('assert/strict');
const fs = require('fs');
const path = require('path');
const root = path.resolve(__dirname, '../..');
const out = path.resolve(process.env.TTX_QA_OUT || path.join(root, 'runs/reply-threads'));
const url = process.env.TTX_QA_URL || ('file://' + path.join(root, process.env.TTX_STANDALONE_FILE || 'web/standalone/ttx.html'));

const INJECT = 'Ore 14:20, il fronte ha scavalcato il crinale in E3 e scende verso F4, vento W 25 km/h';
const FIRST = 'Mandiamo due squadre AIB sul lato nord di F4 e apriamo una linea tagliafuoco';
const SECOND = 'Chiudiamo la provinciale a F4';

function reading(user, gazetteer) {
  const road = (gazetteer.match(/"roads?"\s*:\s*\[\s*\{[^}]*?"id"\s*:\s*"([^"]+)"/) || [])[1] || 'sp1';
  if (user.includes(SECOND)) return {
    actions: [{ type: 'CloseRoad', role: 'fire_commander', params: { road }, quote: SECOND }],
    questions: [], summary: 'Road closure.' };
  return {
    actions: [
      { type: 'DeployEngine', role: 'fire_commander', params: { at: 'F4' }, quote: 'Mandiamo due squadre AIB sul lato nord di F4' },
      { type: 'CutFireLine', role: 'fire_commander', params: {}, quote: 'apriamo una linea tagliafuoco' }],
    questions: [{ to: 'fire_commander', text: 'Per la linea tagliafuoco: tra quali punti?', about: [1] }],
    summary: 'A team and a fire line without endpoints.' };
}

(async () => {
  fs.mkdirSync(out, { recursive: true });
  const errors = [];
  const browser = await chromium.launch({ headless: true, channel: process.env.TTX_BROWSER_CHANNEL || 'chrome' });
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 }, deviceScaleFactor: 1 });
  page.on('pageerror', e => { if (!e.message.includes('control flow')) errors.push(e.message); });
  await page.route('https://openrouter.ai/api/v1/chat/completions', async route => {
    const req = route.request().postDataJSON();
    const system = req.messages?.[0]?.content || '';
    const user = req.messages?.at(-1)?.content || '';
    const content = system.includes('Players never act on the simulator') ? JSON.stringify(reading(user, system)) : user.replace(/^.*?:\n/s, m => m.length < 80 ? '' : m);
    await route.fulfill({ status: 200, contentType: 'application/json', headers: { 'Access-Control-Allow-Origin': '*' },
      body: JSON.stringify({ choices: [{ message: { content } }] }) });
  });
  const panel = role => page.locator(`[data-panel="${role}"]`);
  const choose = role => page.locator(`.role-tabs [data-role="${role}"]`).click();
  const master = panel('master'), fire = panel('fire_commander');
  const shot = async name => { await page.waitForTimeout(300); await page.screenshot({ path: path.join(out, name + '.png') }); };
  const board = master.locator('#review-board');
  const toBoard = async () => { await choose('master'); await master.locator('[data-view="risposte"]').click(); await board.waitFor(); };
  const reply = async text => {
    await choose('fire_commander');
    await fire.locator('[data-view="map"]').click();
    const back = fire.locator('[data-inject-back]');
    if (await back.count()) await back.click();
    await fire.locator('[data-inject-row]').first().click();
    // One composer under the inject: reply to the inject's sender, one Send.
    const box = fire.locator('[data-reply-composer]').first();
    await box.locator('[data-to="sender"]').check();
    const field = box.locator('#response-narrative');
    await field.click();
    await field.pressSequentially(text.slice(0, 1));
    await field.fill(text);
    await box.locator('[data-reply-send]:not([disabled])').waitFor();
    await box.locator('[data-reply-send]').click();
    await fire.locator('#response-narrative').first().waitFor();
    await page.waitForFunction(() => !document.querySelector('[data-reply-composer] #response-narrative')?.value);
  };
  const open = () => board.locator(':scope > [data-review]');
  const handled = () => board.locator('.rb-handled [data-review]');
  try {
    await page.goto(url, { waitUntil: 'domcontentloaded' });
    if (url.startsWith('http')) { await page.evaluate(() => { localStorage.clear(); sessionStorage.clear(); }); await page.goto(url, { waitUntil: 'domcontentloaded' }); }
    await page.locator('.launcher').waitFor({ timeout: 90000 });
    await page.locator('#launch-openrouter-key').fill('qa-fixture-only');
    await page.locator('#launch-save-key').click();
    await page.locator('.launcher #start-session').click();
    await master.locator('#start').waitFor({ timeout: 60000 });
    const sim = page.getByLabel('Simulazione', { exact: true }); if (await sim.count() && await sim.isChecked()) await sim.uncheck();
    await require('./standalone-languages.cjs').confirmLanguages(page, { fire_commander: 'it', mayor: 'it' });
    await master.locator('#start').click();
    await master.locator('#turn').filter({ hasText: /orders|ordini/ }).waitFor();
    await master.locator('[data-view="timeline"]').click();
    await master.locator('[data-inject-all]').click();
    const modal = page.getByRole('dialog');
    await modal.locator('#inject-title').fill('Il fronte scavalca il crinale');
    await modal.locator('#inject-sender').fill('Sala operativa');
    await modal.locator('#text').fill(INJECT);
    await modal.locator('#inject-scope').selectOption('custom');
    await modal.getByLabel(/Comandante|Commander|DOS/).first().check();
    await modal.locator('#send').click();
    await modal.waitFor({ state: 'hidden' });

    // 1. First reply: one open group, pending.
    await reply(FIRST);
    await toBoard();
    await board.locator('[data-question-state="draft"]').waitFor({ timeout: 20000 });
    assert.equal(await open().count(), 1);
    const badge = () => master.locator('[data-view="risposte"] .unread').count();
    assert.equal(await badge(), 1, 'the reply awaits the EXCON');
    await shot('01-first-reply-open');

    // 2. EXCON handles everything: queue the team (any feasible place), ignore the line, discard the question.
    const team = board.locator('[data-review-item]').filter({ hasText: /squadr|team|Engine/i }).first();
    for (const place of ['F4', 'Pineta', 'Valverde', 'E5', 'F5', 'E6']) {
      if (await team.getAttribute('data-review-state') !== 'complete') {
        if (!(await team.locator('[data-review-field$=":at"]').count())) await team.locator('[data-review-details]').click();
        await team.locator('[data-review-field$=":at"]').fill(place);
      }
      await team.locator('[data-review-execute]').click();
      await page.waitForTimeout(500);
      if (['queued', 'executed'].includes(await team.getAttribute('data-review-state'))) break;
    }
    await board.locator('[data-review-ignore]').first().click();
    await page.waitForTimeout(300);
    await board.locator('[data-review-discard]').first().click();
    await page.waitForTimeout(1500);
    await shot('02a-after-discard');
    await master.locator('[data-view="timeline"]').click(); await toBoard();
    await handled().first().waitFor({ state: 'attached', timeout: 10000 });
    assert.equal(await open().count(), 0, 'the handled reply left the open list');
    assert.equal(await badge(), 0, 'no reply awaits the EXCON');
    assert.equal(await board.locator('[data-review-count]:not([hidden])').count(), 0, 'no count left on the board');
    await shot('02-first-reply-handled');

    // 3. Second reply: a new distinct group; the first stays handled and closed.
    await reply(SECOND);
    await toBoard();
    await open().filter({ hasText: /provinciale/ }).locator('[data-review-item]').first().waitFor({ timeout: 20000 });
    assert.equal(await open().count(), 1, 'only the new reply is open');
    assert.equal(await handled().count(), 1, 'the old reply stays handled');
    assert.equal(await open().first().locator('[data-review-item]').count(), 1, 'no suggestion of the first reply is merged');
    await master.locator('.rb-handled > summary').click();
    await shot('03-second-reply-distinct');
    assert.deepEqual(errors, []);
    console.log('reply-threads QA ok:', out);
  } catch (e) {
    await page.screenshot({ path: path.join(out, 'failure.png') });
    console.error(e); console.error(errors);
    process.exitCode = 1;
  } finally { await browser.close(); }
})();

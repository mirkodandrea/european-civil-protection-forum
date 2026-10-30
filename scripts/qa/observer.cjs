// Standalone QA of the Observers dashboard ("Osservatori" tab): a two-turn
// session (custom and MSEL injects, replies on different bases, a player
// conversation, one AI suggestion executed), then the dashboard overall, for
// one player and cumulated. The AI is a scripted fixture (no external model).
// Screenshots go to TTX_QA_OUT (default runs/observer-review).
//   TTX_QA_URL=http://127.0.0.1:8093/ node scripts/qa/observer.cjs
const { chromium } = require(process.env.TTX_PLAYWRIGHT || 'playwright');
const assert = require('assert/strict');
const fs = require('fs');
const path = require('path');
const root = path.resolve(__dirname, '../..');
const out = path.resolve(process.env.TTX_QA_OUT || path.join(root, 'runs/observer-review'));
const url = process.env.TTX_QA_URL || ('file://' + path.join(root, 'web/standalone/ttx.html'));

const WARN = 'Diramiamo l’allerta alla popolazione in F4';
const DOS = 'Mandiamo due squadre AIB sul lato nord di F4. ' + WARN;

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
    let content = user.replace(/^.*?:\n/s, m => m.length < 80 ? '' : m);
    if (system.includes('Players never act on the simulator')) content = JSON.stringify(user.includes(WARN)
      ? { actions: [{ type: 'IssueWarning', role: 'mayor', params: { zone: 'F4' }, quote: WARN }], questions: [], summary: 'Warning in F4.' }
      : { actions: [], questions: [], summary: 'Nothing to do.' });
    else if (system.startsWith('Translate')) content = user;
    await route.fulfill({ status: 200, contentType: 'application/json', headers: { 'Access-Control-Allow-Origin': '*' },
      body: JSON.stringify({ choices: [{ message: { content } }] }) });
  });
  const panel = role => page.locator(`[data-panel="${role}"]`);
  const choose = role => page.locator(`.role-tabs [data-role="${role}"]`).click();
  const master = panel('master'), fire = panel('fire_commander'), mayor = panel('mayor'), obs = panel('observer');
  /** Screenshot of `el` whole: the viewport grows to its content, then returns. */
  const shot = async (name, el) => {
    await page.waitForTimeout(600);
    const size = page.viewportSize();
    const h = await el.locator('#observer').evaluate(e => e.scrollHeight).catch(() => size.height);
    await page.setViewportSize({ width: size.width, height: Math.min(Math.max(size.height, h + 160), 5000) });
    await page.waitForTimeout(300);
    await el.screenshot({ path: path.join(out, name + '.png') });
    await page.setViewportSize(size);
  };
  const openInject = async (p, role, n = 0) => {
    await choose(role);
    await p.locator('[data-view="map"]').click();
    const back = p.locator('[data-inject-back]');
    if (await back.count()) await back.click();
    await p.locator('[data-inject-row]').nth(n).click();
  };
  const reply = async (p, text, basis, note) => {
    const work = p.locator('[data-reply-composer]').first();
    await work.locator('[data-to="sender"]').check();
    await work.locator('#response-narrative').fill(text);
    await work.locator(`#basis-${basis}`).click();
    if (note) await work.locator('[data-basis-note]').fill(note);
    await work.locator('[data-rationale]').fill('Il fronte si avvicina alle case.');
    await work.locator('[data-respond]').click();
    await p.locator('[data-response-state="rs_sent"]').first().waitFor();
  };
  const turnIs = n => master.locator('#turn').filter({ hasText: new RegExp(`\\b${n}\\b.*(orders|ordini)`, 'i') });
  /** EXCON sends a custom inject to the roles whose checkbox labels match. */
  const sendCustom = async (title, text, recipients) => {
    await choose('master');
    await master.locator('[data-view="timeline"]').click();
    await master.locator('[data-inject-all]').click();
    const modal = page.getByRole('dialog');
    await modal.locator('#inject-title').fill(title);
    await modal.locator('#inject-sender').fill('Sala operativa');
    await modal.locator('#text').fill(text);
    await modal.locator('#inject-scope').selectOption('custom');
    for (const r of recipients) await modal.getByLabel(r).first().check();
    await modal.locator('#send').click();
    await modal.waitFor({ state: 'hidden' });
  };
  /** Advances until turn `n` opens, sending every due inject on the way. */
  const advanceTo = async n => {
    await choose('master');
    await master.locator('[data-view="timeline"]').click();
    for (let k = 0; k < 12 && !(await turnIs(n).count()); k++) {
      await master.locator('#advance-next:not([disabled])').click();
      const confirm = page.locator('[data-due-confirm]');
      await Promise.race([confirm.waitFor({ timeout: 4000 }).catch(() => {}), page.waitForTimeout(1500)]);
      if (await confirm.count()) {
        while (await page.locator('[data-review-send]:not([disabled])').count()) {
          await page.locator('[data-review-send]:not([disabled])').first().click();
          await page.waitForTimeout(500);
        }
        await page.locator('[data-due-confirm]:not([disabled])').click();
      }
      await master.locator('#advance-next:not([disabled])').waitFor({ timeout: 60000 });
    }
    await turnIs(n).waitFor({ timeout: 60000 });
  };
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

    // Before the start: the dashboard waits.
    await choose('observer');
    await obs.locator('#observer').waitFor();
    await shot('00-briefing', obs);

    await choose('master');
    await master.locator('#start').click();
    await turnIs(1).waitFor();

    // Turn 1: a custom inject to the DOS and the Mayor.
    await sendCustom('Il fronte scavalca il crinale', 'Il fronte ha scavalcato il crinale in E3 e scende verso F4.', [/Comandante|Commander|DOS/, /Sindaco|Mayor/]);

    await openInject(fire, 'fire_commander');
    await reply(fire, DOS, 'operational_judgment');
    await openInject(mayor, 'mayor');
    await reply(mayor, 'Apriamo il centro operativo comunale.', 'plan', 'Piano comunale di protezione civile, scheda 2');
    // The Mayor asks the DOS about the inject; the DOS answers.
    const work = mayor.locator('[data-reply-composer]').first();
    await work.locator('[data-to="players"]').check();
    await work.locator('[data-conv-person="fire_commander"]').check();
    await work.locator('#response-narrative').fill('Serve evacuare F4 adesso?');
    // Starting a conversation needs its reason (basis + rationale).
    await work.locator('#basis-operational_judgment').click();
    await work.locator('[data-rationale]').fill('Prima di decidere sull’evacuazione serve il parere del DOS.');
    await work.locator('[data-send-conversation]:not([disabled])').waitFor();
    await work.locator('[data-send-conversation]').click();
    await openInject(fire, 'fire_commander');
    await fire.locator('[data-conv-row]').first().click();
    await fire.locator('#response-narrative').fill('Sì, entro 20 minuti.');
    await fire.locator('[data-send-conversation]').click();
    await fire.locator('[data-conversation] [data-conv-message]').nth(1).waitFor();

    // The EXCON executes the AI suggestion read in the DOS reply.
    await choose('master');
    await master.locator('[data-view="risposte"]').click();
    const execute = master.locator('#review-board [data-review-execute]').first();
    await execute.waitFor({ timeout: 20000 });
    await execute.click();
    await page.waitForTimeout(800);

    await choose('observer');
    await obs.locator('[data-observer-turn="1"].active').waitFor();
    await shot('01-turn1-live', obs);
    assert.equal(await obs.locator('[data-kpi="responses"] .obs-kpi-value').innerText(), '2');
    assert.equal(await obs.locator('[data-kpi="interactions"] .obs-kpi-value').innerText(), '2');

    // Turn 2: MSEL injects were sent as they came due; the DOS answers a new one.
    await advanceTo(2);
    await sendCustom('Richiesta di rinforzi', 'Il presidio nord chiede una terza squadra.', [/Comandante|Commander|DOS/]);
    await openInject(fire, 'fire_commander', 0);
    await reply(fire, 'Rinforziamo il presidio con una terza squadra.', 'law', 'L.R. 39/2000, art. 70');

    await choose('observer');
    await obs.locator('[data-observer-turn="1"]').click();
    await shot('02-turn1-closed', obs);
    await obs.locator('[data-observer-turn="2"]').click();
    await obs.locator('[data-observer-mode="cumulative"]').click();
    await shot('03-cumulative', obs);
    assert.equal(await obs.locator('[data-kpi="responses"] .obs-kpi-value').innerText(), '3');
    await obs.locator('[data-observer-role="fire_commander"]').click();
    await shot('04-player', obs);
    assert.equal(await obs.locator('[data-kpi="responses"] .obs-kpi-value').innerText(), '2');
    assert.equal(await obs.locator('[data-kpi="suggested"] .obs-kpi-value').innerText(), '1');
    await page.setViewportSize({ width: 1180, height: 900 });
    await obs.locator('[data-observer-role=""]').click();
    await shot('05-narrow', obs);
    assert.deepEqual(errors, []);
    console.log('observer QA ok →', out);
  } catch (e) {
    await page.screenshot({ path: path.join(out, 'failure.png'), fullPage: true }).catch(() => {});
    console.error(e, errors);
    process.exitCode = 1;
  } finally {
    await browser.close();
  }
})();

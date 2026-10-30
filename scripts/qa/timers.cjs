// Exercise timers in the full standalone app (file:// build, or TTX_QA_URL for a
// Trunk server): real time since the start and in the turn, turn N/M with the
// simulated time of the turn, the scenario bar. A page reload resumes the saved
// session and the real-time counter continues from the recorded start.
const {chromium} = require(process.env.TTX_PLAYWRIGHT || 'playwright');
const assert = require('assert/strict');
const fs = require('fs');
const path = require('path');

const url = process.env.TTX_QA_URL || ('file://' + path.resolve(process.env.TTX_STANDALONE_FILE || 'web/standalone/ttx.html'));
const out = process.env.TTX_QA_OUT || path.join('runs/timers', new Date().toISOString().replace(/[:.]/g, '-'));

/** The launcher shown before the exercise: resume the saved session, else start the selected scenario. */
async function launch(page) {
  await page.locator('.launcher, [data-panel="master"] #turn-controls').first().waitFor({state: 'attached', timeout: 90000});
  const button = page.locator('.launcher #resume-session, .launcher #start-session').first();
  if (await button.count()) await button.click();
}

(async () => {
  fs.mkdirSync(out, {recursive: true});
  const browser = await chromium.launch({headless: true, ...(process.env.TTX_BROWSER_EXECUTABLE
    ? {executablePath: process.env.TTX_BROWSER_EXECUTABLE}
    : {channel: process.env.TTX_BROWSER_CHANNEL || 'chrome'}),
    args: process.platform === 'linux' ? ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] : []});
  const page = await browser.newPage({viewport: {width: 1680, height: 1000}});
  const errors = [];
  page.on('pageerror', e => {
    if (!e.message.startsWith('Using exceptions for control flow')) errors.push(e.message);
  });
  const master = page.locator('[data-panel="master"]');
  const timers = master.locator('#exercise-timers');
  const elapsed = async () => Number(await timers.locator('#timer-elapsed').getAttribute('data-elapsed-s'));
  const shot = name => page.screenshot({path: path.join(out, name), clip: {x: 0, y: 0, width: 1680, height: 320}});
  try {
    await page.goto(url);
    if (url.startsWith('http')) { await page.evaluate(() => { localStorage.clear(); sessionStorage.clear(); }); await page.goto(url); }
    await page.locator('.launcher').waitFor({timeout: 90000});
    await launch(page);
    await master.locator('#start').waitFor({timeout: 60000});
    assert.equal(await timers.count(), 0, 'no timers before the start');
    await require('./standalone-languages.cjs').confirmLanguages(page);
    await master.locator('#start').click();
    await timers.waitFor({timeout: 30000});

    // Turn 1 of the planned total, nothing simulated yet.
    const turn = await timers.locator('#timer-turn').innerText();
    assert.match(turn, /^(Turno|Turn) 1\/\d+ · 0:00 \/ 30:00 (simulati|simulated)$/, turn);
    assert.equal(await timers.locator('#timer-scenario').count(), 1, 'the scenario bar is shown');
    const first = await elapsed();
    await page.waitForTimeout(3200);
    const later = await elapsed();
    assert(later >= first + 2 && later <= first + 5, `the real-time counter advances: ${first} → ${later}`);
    await shot('timers-started.png');
    console.log('timers shown:', turn, `elapsed ${first}s → ${later}s`);

    // An intermediate advance moves the simulated time inside the turn.
    await master.locator('[data-view="timeline"]').click();
    await master.locator('#advance-step').fill('5');
    await master.locator('#advance-step-go').click();
    const confirm = page.locator('[data-due-confirm]');
    if (await confirm.waitFor({timeout: 5000}).then(() => true, () => false)) {
      for (let i = 0; i < 20; i++) { const on = page.locator('[data-pre-toggle]:checked'); if (!await on.count()) break; await on.first().uncheck(); await page.waitForTimeout(200); }
      await confirm.click();
    }
    await timers.locator('#timer-turn').filter({hasText: /· 5:00 \//}).waitFor({timeout: 60000});
    await master.locator('#turn').filter({hasText: /orders|ordini|Ordini|Orders/}).waitFor({timeout: 60000});
    const advanced = await timers.locator('#timer-turn').innerText();
    const bar = await timers.locator('.timer-bar > span').getAttribute('style');
    assert(!/width:0\.0%/.test(bar), `the scenario bar moves: ${bar}`);
    await shot('timers-advanced.png');
    console.log('after the advance:', advanced, bar);

    // Reload: the saved session resumes and the counter continues from the recorded start.
    await page.waitForTimeout(2000);
    const before = await elapsed();
    await page.reload();
    await launch(page);
    await timers.waitFor({timeout: 90000});
    await page.waitForTimeout(1500);
    const after = await elapsed();
    assert(after >= before + 2, `the elapsed time survives the reload: ${before}s → ${after}s`);
    assert(after <= before + 120, `no jump after the reload: ${before}s → ${after}s`);
    assert.equal(await timers.locator('#timer-turn').innerText(), advanced, 'the turn progress is restored');
    await shot('timers-reloaded.png');
    console.log(`reloaded: elapsed ${before}s → ${after}s`);

    assert.deepEqual(errors, []);
    fs.writeFileSync(path.join(out, 'manifest.json'), JSON.stringify({fixture: 'standalone timers', turn, advanced, first, later, before, after, errors}, null, 2));
    console.log('OK', out);
  } catch (e) {
    await page.screenshot({path: path.join(out, 'failure.png')}).catch(() => {});
    console.error(e);
    process.exitCode = 1;
  } finally {
    await browser.close();
  }
})();

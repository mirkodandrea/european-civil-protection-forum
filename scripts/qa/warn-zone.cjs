// QA for the visible effect of "Avvisa zona" (IssueWarning): the EXCON warns a
// zone from Controllo ("Subito"), then the warned zone (hatching, bell label
// with time) must appear on the EXCON maps and on the player's map, and the
// operation notice must say what happened. Screenshots go to TTX_QA_OUT.
// Usage: TTX_QA_URL=http://127.0.0.1:8084 node scripts/qa/warn-zone.cjs
const { chromium } = require(process.env.TTX_PLAYWRIGHT || 'playwright');
const fs = require('fs');
const path = require('path');
const assert = require('assert/strict');

const cwd = path.resolve(__dirname, '../..');
const out = path.resolve(process.env.TTX_QA_OUT || path.join(cwd, 'runs/warn-zone-review'));
const base = process.env.TTX_QA_URL || 'http://127.0.0.1:8080';

(async () => {
  fs.mkdirSync(out, { recursive: true });
  const errors = [];
  const browser = await chromium.launch({ headless: true, channel: process.env.TTX_BROWSER_CHANNEL || 'chrome' });
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 }, deviceScaleFactor: 1 });
  page.on('pageerror', e => { if (!e.message.includes('control flow')) errors.push(e.message); });
  await page.route('https://openrouter.ai/api/v1/chat/completions', async route => {
    const req = route.request().postDataJSON();
    await route.fulfill({ status: 200, contentType: 'application/json', headers: { 'Access-Control-Allow-Origin': '*' },
      body: JSON.stringify({ choices: [{ message: { content: req.messages?.at(-1)?.content || '' } }] }) });
  });
  // Record bell labels drawn on any canvas.
  await page.addInitScript(() => {
    const original = CanvasRenderingContext2D.prototype.fillText;
    CanvasRenderingContext2D.prototype.fillText = function (text, ...rest) {
      if (String(text).includes('🔔')) (window.__bells ||= []).push({ canvas: this.canvas?.id || '', text: String(text) });
      return original.call(this, text, ...rest);
    };
  });
  const shot = async name => { await page.waitForTimeout(600); await page.screenshot({ path: path.join(out, name + '.png') }); };
  const master = page.locator('[data-panel="master"]');
  try {
    await page.goto(base, { waitUntil: 'domcontentloaded' });
    await page.locator('.launcher, [data-panel="master"] #turn-controls').first().waitFor({ state: 'attached', timeout: 120000 });
    if (await page.locator('#launch-openrouter-key').count()) {
      await page.locator('#launch-openrouter-key').fill('qa-fixture-only');
      await page.locator('#launch-save-key').click();
    }
    await page.locator('.launcher #start-session').click();
    await master.locator('#start').waitFor({ timeout: 60000 });
    const sim = page.getByLabel('Simulazione', { exact: true }); if (await sim.count() && await sim.isChecked()) await sim.uncheck();
    await require('./standalone-languages.cjs').confirmLanguages(page, { fire_commander: 'it', mayor: 'it' });
    await master.locator('#start').click();
    await master.locator('#turn').filter({ hasText: /orders|ordini/ }).waitFor();

    // Controllo -> Avvisa zona: tip explains the effect.
    await master.locator('[data-view="map"]').click();
    await master.locator('[data-tool="warn"]').click();
    const modal = page.getByRole('dialog');
    await modal.waitFor();
    const tip = await modal.locator('p.muted').first().innerText();
    assert.match(tip, /10 min/, tip);
    await modal.getByLabel(/Subito, a turno aperto|Right now/).check();
    const canvas = modal.locator('canvas').first();
    const box = await canvas.boundingBox();
    // F4-ish: column F of 8, row 4 of 8 (the map shows the whole territory at overview).
    await canvas.click({ position: { x: box.width * 5.5 / 8, y: box.height * 3.5 / 8 } });
    const picked = await modal.locator('.ctl-pick').innerText();
    await shot('1-warn-tool');
    await modal.locator('[data-apply]').click();
    await modal.waitFor({ state: 'detached', timeout: 20000 });
    const notice = await master.locator('.notice').first().innerText();
    console.log('picked:', picked, '| notice:', notice);
    assert.match(notice, /allertata|warned/, notice);
    await shot('2-excon-control-after');

    await master.locator('[data-view="situazione"]').click();
    await shot('3-excon-situation');
    // Player map (mayor).
    await page.locator('.role-tabs [data-role="mayor"]').click();
    await page.locator('[data-panel="mayor"] [data-view="map"]').click().catch(() => {});
    await shot('4-mayor-map');
    const bells = await page.evaluate(() => window.__bells || []);
    console.log('bell labels:', JSON.stringify([...new Set(bells.map(b => b.canvas + ': ' + b.text))]));
    assert.ok(bells.length > 0, 'no warned-zone label drawn');
    // In-process 3D view (outline + hatching + "WARNED F4 · time" label).
    if (await sim.count()) {
      await page.locator('.role-tabs [data-role="master"]').click();
      await sim.check();
      await page.waitForTimeout(15000);
      await shot('5-3d-view');
    }
    assert.deepEqual(errors, []);
    console.log('OK', out);
  } catch (e) {
    await page.screenshot({ path: path.join(out, 'failure.png') });
    console.error(e); process.exitCode = 1;
  } finally { await browser.close(); }
})();

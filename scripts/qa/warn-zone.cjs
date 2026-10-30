// QA for the visible effect of "Avvisa zona" (IssueWarning) under the fog of
// war: the EXCON warns a zone from Controllo ("Subito"); the warned zone
// (hatching, bell label with time) must appear on the EXCON maps at once, and
// the operation notice must say what happened. The player (mayor) must NOT see
// it until the EXCON reveals that zone to the mayor with the "Edifici"
// (buildings) layer: a reveal with the fire layer only still shows no bell.
// Screenshots go to TTX_QA_OUT.
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
  // Record bell labels drawn on any canvas, with the panel that owns the canvas.
  await page.addInitScript(() => {
    const original = CanvasRenderingContext2D.prototype.fillText;
    CanvasRenderingContext2D.prototype.fillText = function (text, ...rest) {
      if (String(text).includes('🔔')) {
        const panel = this.canvas?.closest?.('[data-panel]')?.dataset?.panel || '';
        (window.__bells ||= []).push({ panel, canvas: this.canvas?.id || '', text: String(text) });
      }
      return original.call(this, text, ...rest);
    };
  });
  const shot = async name => { await page.waitForTimeout(600); await page.screenshot({ path: path.join(out, name + '.png') }); };
  const master = page.locator('[data-panel="master"]');
  // F4-ish: column F of 8, row 4 of 8 (the pick maps show the whole territory at overview).
  const clickF4 = async modal => {
    const canvas = modal.locator('canvas').first();
    const box = await canvas.boundingBox();
    await canvas.click({ position: { x: box.width * 5.5 / 8, y: box.height * 3.5 / 8 } });
  };
  // Bell labels drawn on the mayor's own map since the last reset.
  const mayorBells = async () => {
    await page.evaluate(() => { window.__bells = []; });
    await page.locator('.role-tabs [data-role="mayor"]').click();
    await page.locator('[data-panel="mayor"] [data-view="map"]').click().catch(() => {});
    await page.waitForTimeout(2500);
    const bells = await page.evaluate(() => window.__bells || []);
    return [...new Set(bells.filter(b => b.panel === 'mayor').map(b => b.canvas + ': ' + b.text))];
  };
  const reveal = async (zoneHint, keep) => {
    await page.locator('.role-tabs [data-role="master"]').click();
    await master.locator('[data-view="map"]').click();
    await master.locator('[data-tool="knowledge"]').click();
    const modal = page.getByRole('dialog');
    await modal.waitFor();
    await modal.locator('#report-role').selectOption('mayor');
    for (const box of await modal.locator('fieldset', { hasText: /Livelli da rivelare|Layers to reveal/ }).locator('input[type="checkbox"]').all()) {
      const label = (await box.locator('xpath=..').innerText()).trim();
      if (keep.includes(label) !== await box.isChecked()) await box.locator('xpath=..').click();
    }
    await clickF4(modal);
    const chosen = await modal.locator('p', { hasText: /zone selezionate|zones selected/ }).innerText();
    assert.ok(chosen.includes(zoneHint), `reveal picked ${chosen}, warned ${zoneHint}`);
    return modal;
  };
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
    await clickF4(modal);
    const picked = await modal.locator('.ctl-pick').innerText();
    await shot('1-warn-tool');
    await modal.locator('[data-apply]').click();
    await modal.waitFor({ state: 'detached', timeout: 20000 });
    const notice = await master.locator('.notice').first().innerText();
    console.log('picked:', picked, '| notice:', notice);
    assert.match(notice, /allertata|warned/, notice);
    const zone = (notice.match(/\b([A-H][1-8])\b/) || [])[1];
    assert.ok(zone, 'warned zone in the notice: ' + notice);
    await shot('2-excon-control-after');
    await master.locator('[data-view="situazione"]').click();
    await shot('3-excon-situation');

    // Fog of war: the mayor has been told nothing yet.
    let bells = await mayorBells();
    console.log('mayor bells before any reveal:', JSON.stringify(bells));
    await shot('4-mayor-map-before-reveal');
    assert.deepEqual(bells, [], 'mayor sees the warning before being told');

    // Reveal the zone with the fire layer only: still no bell.
    let reveal1 = await reveal(zone, ['Incendio', 'Fire']);
    await shot('5-reveal-fire-only');
    await reveal1.locator('[data-apply]').click();
    await reveal1.waitFor({ state: 'detached', timeout: 20000 });
    bells = await mayorBells();
    console.log('mayor bells after fire-only reveal:', JSON.stringify(bells));
    await shot('6-mayor-map-fire-only');
    assert.deepEqual(bells, [], 'a fire-only reveal disclosed the warning');

    // Reveal it with the buildings layer: the bell appears on the mayor's map.
    const reveal2 = await reveal(zone, ['Edifici', 'Buildings']);
    await shot('7-reveal-buildings');
    await reveal2.locator('[data-apply]').click();
    await reveal2.waitFor({ state: 'detached', timeout: 20000 });
    bells = await mayorBells();
    console.log('mayor bells after buildings reveal:', JSON.stringify(bells));
    await shot('8-mayor-map-after-reveal');
    assert.ok(bells.some(b => b.includes(zone)), 'no warned-zone label on the mayor map');

    // In-process 3D view (outline + hatching + "WARNED F4 · time" label).
    if (await sim.count()) {
      await page.locator('.role-tabs [data-role="master"]').click();
      await sim.check();
      await page.waitForTimeout(15000);
      await shot('9-3d-view');
    }
    assert.deepEqual(errors, []);
    console.log('OK', out);
  } catch (e) {
    await page.screenshot({ path: path.join(out, 'failure.png') });
    console.error(e); process.exitCode = 1;
  } finally { await browser.close(); }
})();

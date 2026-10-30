// QA for "Vedi come <ruolo>" in Controllo -> "Informazioni ai ruoli": the EXCON
// sees a role's map exactly as its player does. Before any reveal the role's map
// is fogged; the selected zones are previewed on it; after the reveal the same
// view (dialog still open) shows the zone, and the player's own map agrees.
// Screenshots go to TTX_QA_OUT.
// Usage: TTX_QA_URL=http://127.0.0.1:8093 node scripts/qa/view-as-role.cjs
const { chromium } = require(process.env.TTX_PLAYWRIGHT || 'playwright');
const fs = require('fs');
const path = require('path');
const assert = require('assert/strict');

const cwd = path.resolve(__dirname, '../..');
const out = path.resolve(process.env.TTX_QA_OUT || path.join(cwd, 'runs/view-as-role-review'));
const base = process.env.TTX_QA_URL || 'http://127.0.0.1:8080';
const ROLE = 'mayor';

/** RGBA bytes of a canvas, sampled on a coarse grid (enough to compare states). */
async function canvasSample(page, selector) {
  return page.evaluate(sel => {
    const c = document.querySelector(sel);
    if (!c || !c.width) return null;
    const ctx = c.getContext('2d');
    const d = ctx.getImageData(0, 0, c.width, c.height).data;
    const out = [];
    const step = Math.max(1, Math.floor(c.width / 160));
    for (let y = 0; y < c.height; y += step) for (let x = 0; x < c.width; x += step) {
      const i = (y * c.width + x) * 4;
      out.push(d[i], d[i + 1], d[i + 2]);
    }
    return out;
  }, selector);
}
function changedFraction(a, b) {
  let n = 0;
  for (let i = 0; i < a.length; i += 3) {
    if (Math.abs(a[i] - b[i]) + Math.abs(a[i + 1] - b[i + 1]) + Math.abs(a[i + 2] - b[i + 2]) > 30) n++;
  }
  return n / (a.length / 3);
}

(async () => {
  fs.mkdirSync(out, { recursive: true });
  const errors = [];
  const browser = await chromium.launch({ headless: true, channel: process.env.TTX_BROWSER_CHANNEL || 'chrome' });
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 }, deviceScaleFactor: 1 });
  page.on('pageerror', e => { if (!e.message.includes('control flow')) errors.push(e.message); });
  // The AI only translates here: echo the text back.
  await page.route('https://openrouter.ai/api/v1/chat/completions', async route => {
    const req = route.request().postDataJSON();
    await route.fulfill({ status: 200, contentType: 'application/json', headers: { 'Access-Control-Allow-Origin': '*' },
      body: JSON.stringify({ choices: [{ message: { content: req.messages?.at(-1)?.content || '' } }] }) });
  });
  const shot = async name => { await page.waitForTimeout(700); await page.screenshot({ path: path.join(out, name + '.png') }); };
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

    // Controllo -> Informazioni ai ruoli, role = mayor.
    await master.locator('[data-view="map"]').click();
    await master.locator('[data-tool="knowledge"]').click();
    const modal = page.getByRole('dialog');
    await modal.waitFor();
    await modal.locator('#report-role').selectOption(ROLE);
    assert.match(await modal.innerText(), /non conosce ancora nessuna zona|does not know any zone yet/);
    await shot('1-excon-selection');

    // View as the mayor: fogged, nothing known.
    await modal.locator('#view-as').click();
    await modal.locator('#view-as-map').waitFor({ timeout: 20000 });
    assert.equal(await modal.locator('#view-as').getAttribute('aria-pressed'), 'true');
    await page.waitForTimeout(800);
    const fogged = await canvasSample(page, '#view-as-map');
    assert.ok(fogged && fogged.length, 'view-as canvas not drawn');
    await shot('2-view-as-fogged');

    // Click the zone at the centre of the role's map (the territory is centred
    // in the canvas): previewed on the role's map, listed as new.
    const box = await modal.locator('#view-as-map').boundingBox();
    await modal.locator('#view-as-map').click({ position: { x: box.width / 2, y: box.height / 2 } });
    const adds = await modal.locator('#reveal-adds').innerText();
    console.log('preview:', adds);
    assert.match(adds, /Nuove per il ruolo|New to the role/, adds);
    await page.waitForTimeout(500);
    const preview = await canvasSample(page, '#view-as-map');
    const previewDiff = changedFraction(fogged, preview);
    console.log('preview changed fraction:', previewDiff.toFixed(4));
    assert.ok(previewDiff > 0.001, 'selection preview not drawn on the role map');
    await shot('3-view-as-preview');

    // Reveal: the dialog stays open, the selection is cleared and the map updates.
    await modal.locator('[data-apply]').click();
    await modal.getByText(/Zone già note:|Zones already known:/).waitFor({ timeout: 20000 });
    await modal.locator('#reveal-adds').waitFor({ state: 'detached', timeout: 20000 });
    await page.waitForTimeout(1500);
    const revealed = await canvasSample(page, '#view-as-map');
    const revealDiff = changedFraction(fogged, revealed);
    console.log('revealed changed fraction:', revealDiff.toFixed(4), '|', await modal.getByText(/Zone già note:|Zones already known:/).innerText());
    assert.ok(revealDiff > 0.001, 'the role map did not change after the reveal');
    await shot('4-view-as-after-reveal');

    // The EXCON selection tab marks the known zones too.
    await modal.locator('#view-excon').click();
    await shot('5-excon-selection-after');
    await modal.locator('[data-cancel]').click();

    // The player's own map shows the same.
    await page.locator(`.role-tabs [data-role="${ROLE}"]`).click();
    await page.locator(`[data-panel="${ROLE}"] [data-view="map"]`).click().catch(() => {});
    await shot('6-player-map');
    assert.deepEqual(errors, []);
    console.log('OK', out);
  } catch (e) {
    await page.screenshot({ path: path.join(out, 'failure.png') });
    console.error(e); process.exitCode = 1;
  } finally { await browser.close(); }
})();

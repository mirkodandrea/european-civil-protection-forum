// Invisible QA for always-visible zone codes in the tactical reply map.
// Uses a real EXCON inject and the ordinary player UI; no simulator state is mutated.
const { chromium } = require(process.env.TTX_PLAYWRIGHT || '../record/node_modules/playwright');
const fs = require('fs');
const path = require('path');
const assert = require('assert/strict');

/** The launcher shown before the exercise: resume the saved session, else start the selected scenario. */
async function launch(page) {
  await page.locator('.launcher, [data-panel="master"] #turn-controls').first().waitFor({ state: 'attached', timeout: 60000 });
  const button = page.locator('.launcher #resume-session, .launcher #start-session').first();
  if (await button.count()) await button.click();
}

const cwd = path.resolve(__dirname, '../..');
const out = path.resolve(process.env.TTX_QA_OUT || path.join(cwd, 'runs/map-zone-labels-review'));
const base = process.env.TTX_QA_URL || 'http://127.0.0.1:8080';

(async () => {
  fs.mkdirSync(out, { recursive: true });
  const browser = await chromium.launch({ headless: true, channel: 'chrome' });
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 }, deviceScaleFactor: 1 });
  const errors = [], runtimeSignals = [];
  page.on('pageerror', error => {
    if (error.message === "Using exceptions for control flow, don't mind me. This isn't actually an error!") runtimeSignals.push('winit event-loop control flow');
    else errors.push(error.message);
  });
  await page.route('https://openrouter.ai/api/v1/chat/completions', async route => {
    const request = route.request().postDataJSON();
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      headers: { 'Access-Control-Allow-Origin': '*' },
      body: JSON.stringify({ choices: [{ message: { content: request.messages?.at(-1)?.content || '' } }] }),
    });
  });
  await page.addInitScript(() => {
    const original = CanvasRenderingContext2D.prototype.fillText;
    const originalSetTransform = CanvasRenderingContext2D.prototype.setTransform;
    const worldTransforms = new WeakMap();
    CanvasRenderingContext2D.prototype.setTransform = function (...args) {
      const result = originalSetTransform.apply(this, args);
      if (this.canvas?.id === 'work-map') {
        const t = this.getTransform();
        if (Math.abs(t.e) > 0.5 || Math.abs(t.f) > 0.5 || t.a > 3) {
          worldTransforms.set(this, { a: t.a, d: t.d, e: t.e, f: t.f });
        }
      }
      return result;
    };
    CanvasRenderingContext2D.prototype.fillText = function (text, x, y, ...rest) {
      const canvas = this.canvas;
      if (canvas?.id === 'work-map' && /^[A-H][1-8]$/.test(String(text))) {
        const rect = canvas.getBoundingClientRect();
        const t = this.getTransform();
        const cssX = (t.a * x + t.e) * rect.width / canvas.width;
        const cssY = (t.d * y + t.f) * rect.height / canvas.height;
        (window.__zoneCodeDraws ||= []).push({
          code: String(text), font: this.font,
          fontCSS: Number(this.font.match(/([\d.]+)px/)?.[1] || 0) * t.a * rect.width / canvas.width,
          x: cssX, y: cssY, width: rect.width, height: rect.height,
          worldCenter: (() => {
            const world = worldTransforms.get(this);
            if (!world) return null;
            const col = String(text).charCodeAt(0) - 65;
            const row = Number(String(text).slice(1)) - 1;
            const mapX = (col + 0.5) * canvas.width / 8;
            const mapY = (row + 0.5) * canvas.height / 8;
            return {
              x: (world.a * mapX + world.e) * rect.width / canvas.width,
              y: (world.d * mapY + world.f) * rect.height / canvas.height,
            };
          })(),
        });
      }
      return original.call(this, text, x, y, ...rest);
    };
  });
  const master = page.locator('[data-panel="master"]');
  const mayor = page.locator('[data-panel="mayor"]');
  const choose = role => page.locator(`.role-tabs [data-role="${role}"]`).click();
  const draws = () => page.evaluate(() => window.__zoneCodeDraws || []);
  const clearDraws = () => page.evaluate(() => { window.__zoneCodeDraws = []; });
  const waitPaint = () => page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  const assertFontSize = (records, stage) => {
    assert(records.length > 0, `${stage}: instrumented zone codes were drawn`);
    for (const record of records) assert(Math.abs(record.fontCSS - 11) < 0.2, `${stage}: ${record.code} perceived font ${record.fontCSS.toFixed(2)} CSS px`);
  };
  const saveFontEvidence = records => ({
    count: records.length,
    perceivedCssPx: records.map(r => Number(r.fontCSS.toFixed(2))),
    codes: [...new Set(records.map(r => r.code))].sort(),
    canvasCss: records[0] ? [Number(records[0].width.toFixed(1)), Number(records[0].height.toFixed(1))] : null,
  });

  try {
    await page.goto(base, { waitUntil: 'domcontentloaded' }); await launch(page);
    await master.locator('#start').waitFor({ timeout: 30000 });
    await page.locator('.standalone-settings summary').click();
    await page.locator('#openrouter-key').fill('qa-fixture-only');
    await page.locator('#configure-ai').click();
    assert.equal(await page.locator('#openrouter-key').inputValue(), '', 'fixture credential clears after setup');
    await page.locator('.standalone-settings summary').click();
    await page.getByLabel('Simulazione', { exact: true }).uncheck();
    await master.locator('#start').click();
    await master.locator('#turn').filter({ hasText: 'orders' }).waitFor();

    // Deliver a normal inject, then open the player's real reply editor.
    await master.locator('[data-view="timeline"]').click();
    await master.locator('[data-inject-role="mayor"]').click();
    let inject = page.getByRole('dialog');
    await page.waitForFunction(() => document.activeElement?.id === 'inject-title');
    await inject.locator('#inject-title').fill('Zone label review');
    await inject.locator('#text').fill('Coordinate a warning for F4 with the fire commander.');
    await inject.locator('#send').click();
    await inject.waitFor({ state: 'hidden' });
    await choose('mayor');
    await mayor.locator('[data-view="map"]').click();
    await mayor.locator('#player-timeline').getByText('Zone label review').waitFor();
    await mayor.locator('[data-reply-inject]').click();
    const modal = page.locator('.work-modal');
    const canvas = modal.locator('#work-map');
    await canvas.waitFor();
    await waitPaint();
    const overview = await draws();
    assertFontSize(overview, 'overview');
    await page.screenshot({ path: path.join(out, 'map-zone-overview.png'), fullPage: true });

    // Labels remain visible when the optional place-name layer is disabled.
    await modal.locator('.map-layers summary').click();
    await clearDraws();
    await modal.locator('.map-layers label').filter({ hasText: 'Etichette' }).locator('input').uncheck();
    await waitPaint();
    const labelsOff = await draws();
    assertFontSize(labelsOff, 'labels layer off');
    await modal.locator('.map-layers summary').click();

    // Zoom through the actual toolbar, then pan to the map edge. A partially
    // visible edge zone must still receive a code even though its centre is out of view.
    const zoomIn = modal.getByRole('button', { name: 'Ingrandisci' });
    await clearDraws();
    for (let i = 0; i < 7; i++) await zoomIn.click();
    await waitPaint();
    const zoomed = await draws();
    assertFontSize(zoomed, 'high zoom');
    const badge = await modal.locator('.map-zoom-badge').innerText();
    const box = await canvas.boundingBox();
    assert(box, 'work map has a visible bounding box');
    await clearDraws();
    await page.mouse.move(box.x + box.width * 0.68, box.y + box.height * 0.5);
    await page.mouse.down();
    await page.mouse.move(box.x + 4, box.y + box.height * 0.5, { steps: 8 });
    await page.mouse.up();
    await waitPaint();
    const edge = await draws();
    assertFontSize(edge, 'panned edge');
    assert(edge.some(r => r.x < 36 || r.x > r.width - 36), 'a code is drawn near a viewport edge after panning');
    const offscreenCenterCode = edge.find(r => r.worldCenter &&
      (r.worldCenter.x < 0 || r.worldCenter.x > r.width || r.worldCenter.y < 0 || r.worldCenter.y > r.height) &&
      r.x >= 0 && r.x <= r.width && r.y >= 0 && r.y <= r.height);
    assert(offscreenCenterCode, 'a partially visible zone keeps its label in view while its geographic centre is offscreen');
    await page.screenshot({ path: path.join(out, 'map-zone-zoom.png'), fullPage: true });

    await clearDraws();
    await page.setViewportSize({ width: 390, height: 844 });
    await waitPaint();
    const mobile = await draws();
    assertFontSize(mobile, '390px responsive resize');
    await page.screenshot({ path: path.join(out, 'map-zone-mobile.png'), fullPage: true });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false,
      '390px viewport has no horizontal overflow');
    assert.equal(errors.length, 0, errors.join('\n'));

    fs.writeFileSync(path.join(out, 'manifest.json'), JSON.stringify({
      app: base,
      scenario: 'valverde_factory',
      flow: ['start EXCON', 'send real mayor inject', 'open inject reply map', 'instrument visible zone code canvas draws', 'disable labels layer', 'zoom and pan to map edge', 'resize to 390px'],
      evidence: {
        overview: saveFontEvidence(overview),
        labelsLayerOff: saveFontEvidence(labelsOff),
        zoom: saveFontEvidence(zoomed),
        edgeAfterPan: saveFontEvidence(edge),
        mobile: saveFontEvidence(mobile),
        zoomBadge: badge,
        edgeCodePresent: edge.some(r => r.x < 36 || r.x > r.width - 36),
        offscreenZoneCenterCode: offscreenCenterCode && {
          code: offscreenCenterCode.code,
          labelCss: [Number(offscreenCenterCode.x.toFixed(1)), Number(offscreenCenterCode.y.toFixed(1))],
          geographicCenterCss: [Number(offscreenCenterCode.worldCenter.x.toFixed(1)), Number(offscreenCenterCode.worldCenter.y.toFixed(1))],
        },
      },
      viewports: [[1440, 1000], [390, 844]],
      screenshots: ['map-zone-overview.png', 'map-zone-zoom.png', 'map-zone-mobile.png'],
      credentials: 'fixture-only; cleared before use; no credential values stored',
      runtimeSignals,
      errors,
    }, null, 2));
    console.log(`PASS map zone labels QA: ${out}`);
  } catch (error) {
    fs.writeFileSync(path.join(out, 'failure.json'), JSON.stringify({ error: String(error), stack: error.stack, errors }, null, 2));
    try { await page.screenshot({ path: path.join(out, 'failure.png'), fullPage: true }); } catch (_) {}
    throw error;
  } finally {
    await browser.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });

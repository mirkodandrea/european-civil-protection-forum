// Invisible QA for the 2D base map: fuel colours from the scenario's fuel definitions
// (EU12 vs legacy ids) and the sea. Full standalone app (Trunk `--features standalone`,
// TTX_QA_URL, default http://127.0.0.1:8097), one fresh session per embedded scenario pack,
// EXCON "Situazione" map screenshot. AI requests are answered locally; nothing is sent out.
const { chromium } = require(process.env.TTX_PLAYWRIGHT || 'playwright');
const fs = require('fs');
const path = require('path');
const assert = require('assert/strict');
const { confirmLanguages } = require('./standalone-languages.cjs');

const cwd = path.resolve(__dirname, '../..');
const out = path.resolve(process.env.TTX_QA_OUT || path.join(cwd, 'runs/map-fuel-colours'));
const url = process.env.TTX_QA_URL || 'http://127.0.0.1:8097/';
// [pack id, screenshot name, expects sea]
const packs = (process.env.TTX_QA_PACKS || 'valverde_night_wind,selvariva_crisi_integrata,roccavento_valico')
  .split(',').map(id => [id.trim(), /roccavento/.test(id)]);
const SEA = [168, 201, 222];

(async () => {
  fs.mkdirSync(out, { recursive: true });
  const browser = await chromium.launch({ headless: true, channel: process.env.TTX_BROWSER_CHANNEL || 'chrome' });
  const results = [];
  try {
    for (const [pack, sea] of packs) {
      const page = await browser.newPage({ viewport: { width: 1600, height: 1100 }, deviceScaleFactor: 1 });
      page.setDefaultTimeout(90000);
      const errors = [];
      page.on('pageerror', e => { if (!e.message.startsWith('Using exceptions for control flow')) errors.push(e.message); });
      await page.route('https://openrouter.ai/**', route => route.fulfill({
        status: 200, contentType: 'application/json', headers: { 'Access-Control-Allow-Origin': '*' },
        body: JSON.stringify({ choices: [{ message: { content: '{}' } }] }),
      }));
      await page.goto(url);
      await page.evaluate(() => { localStorage.clear(); sessionStorage.clear(); });
      await page.goto(url);
      await page.locator('.launcher').waitFor();
      await page.locator(`.launcher input[name="launch-scenario"][value="${pack}"]`).check();
      await page.locator('.launcher #start-session').click();
      const master = page.locator('[data-panel="master"]');
      await master.locator('#start').waitFor();
      await confirmLanguages(page);
      await master.locator('[data-view="situazione"]').click();
      const canvas = master.locator('.mapbox canvas').first();
      await canvas.waitFor();
      await page.waitForTimeout(1500);
      // Pixel census of the drawn raster: sea colour present only where the territory has sea.
      const stats = await canvas.evaluate((c, seaRgb) => {
        const g = c.getContext('2d');
        const { data } = g.getImageData(0, 0, c.width, c.height);
        let seaPx = 0, grey = 0, n = 0;
        const colours = new Map();
        for (let i = 0; i < data.length; i += 16) {
          const [r, gg, b] = [data[i], data[i + 1], data[i + 2]];
          n++;
          if (Math.abs(r - seaRgb[0]) < 3 && Math.abs(gg - seaRgb[1]) < 3 && Math.abs(b - seaRgb[2]) < 3) seaPx++;
          if (r === 200 && gg === 200 && b === 190) grey++;
          const k = `${r},${gg},${b}`;
          colours.set(k, (colours.get(k) || 0) + 1);
        }
        const top = [...colours.entries()].sort((a, b) => b[1] - a[1]).slice(0, 12);
        return { sea: seaPx / n, unknownGrey: grey / n, top };
      }, SEA);
      const shot = path.join(out, `${pack}-situation.png`);
      await master.locator('.mapbox').first().screenshot({ path: shot });
      results.push({ pack, shot, ...stats, errors });
      console.log(pack, JSON.stringify({ sea: stats.sea.toFixed(3), unknownGrey: stats.unknownGrey.toFixed(3) }));
      if (sea) assert(stats.sea > 0.2, `${pack}: sea should cover a large part of the map`);
      else assert(stats.sea < 0.01, `${pack}: no sea expected`);
      assert(stats.unknownGrey < 0.01, `${pack}: fuel classes drawn as unknown grey`);
      assert.deepEqual(errors, [], `${pack}: page errors`);
      await page.close();
    }
    fs.writeFileSync(path.join(out, 'manifest.json'), JSON.stringify(results, null, 2));
    console.log('PASS map-fuel-colours', out);
  } finally {
    await browser.close();
  }
})().catch(e => { console.error(e); process.exit(1); });

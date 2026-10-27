// Render the burned-in subtitles (one transparent PNG per narration line)
// and the title bar with Chrome, because this ffmpeg build has no text
// rendering (no drawtext / libass).
//
//   node render_subs.js <out_dir>     reads <out_dir>/narration.json, writes <out_dir>/subs/*.png
const { chromium } = require('playwright');
const fs = require('fs');
const path = require('path');

const OUT = process.argv[2];
const W = parseInt(process.env.TTX_SUB_WIDTH || '1380', 10);

(async () => {
  const narr = JSON.parse(fs.readFileSync(path.join(OUT, 'narration.json'), 'utf8'));
  const dir = path.join(OUT, 'subs');
  fs.mkdirSync(dir, { recursive: true });
  const browser = await chromium.launch({ channel: 'chrome' });
  const page = await browser.newPage({ viewport: { width: W, height: 200 }, deviceScaleFactor: 1 });
  const esc = s => s.replace(/[&<>]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' }[c]));
  const shot = async (html, file) => {
    await page.setContent(`<html><body style="margin:0;background:transparent">${html}</body></html>`);
    await page.locator('#box').screenshot({ path: file, omitBackground: true });
  };
  const sub = text => `<div id="box" style="width:${W}px;display:flex;justify-content:center;padding:6px 0">
    <div style="max-width:${W - 80}px;background:rgba(10,10,12,.78);color:#fff;font:500 27px/1.35 -apple-system,system-ui,sans-serif;padding:10px 22px;border-radius:10px;text-align:center">${esc(text)}</div></div>`;
  for (const [i, l] of narr.lines.entries()) {
    await shot(sub(l.text), path.join(dir, `${String(i).padStart(2, '0')}_${l.id}.png`));
  }
  await shot(`<div id="box" style="width:${W}px;display:flex;align-items:center;justify-content:space-between;color:#fff;font:600 30px/1.2 -apple-system,system-ui,sans-serif;padding:8px 4px">
    <span>PPRD East 4 · AI-supported wildfire table-top exercise</span>
    <span style="font-weight:400;font-size:20px;color:#bbb">Valverde valley (synthetic) · live LLM · 32-run Monte Carlo fire</span></div>`, path.join(dir, 'title.png'));
  await browser.close();
  console.log(`subtitles: ${narr.lines.length} lines rendered`);
})().catch(e => { console.error(e); process.exit(1); });

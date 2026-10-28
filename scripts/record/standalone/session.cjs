// Plays a two-turn exercise in the standalone (single-file, in-browser) app with the 3D simulator
// visible, recording the page with the CDP screencast (no visible window needed).
//
//   node standalone/session.cjs <out_dir> [narration.json]
//
// Environment:
//   TTX_STANDALONE_FILE  app to open (default web/standalone/ttx.html)
//   TTX_DRY=1            no recording, no waiting for the voice: screenshots per caption in <out>/dry/
//   TTX_AI=0             skip the OpenRouter key (manual drafts only)
//
// Every caption(id) marks where a narration line starts and waits until the previous one has been
// spoken (like ../session.js), so the voice paces the session; compose.py lays audio and subtitles
// on <out>/marks.json. The OpenRouter key from .env is typed into the (collapsed) settings before
// the recording starts and never appears on screen.
const { chromium } = require('../node_modules/playwright');

/** The launcher shown before the exercise: resume the saved session, else start the selected scenario. */
async function launch(page) {
  await page.locator('.launcher, [data-panel="master"] #turn-controls').first().waitFor({ state: 'attached', timeout: 60000 });
  const button = page.locator('.launcher #resume-session, .launcher #start-session').first();
  if (await button.count()) await button.click();
}
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');

/** The start-of-session language gate; a no-op when this session already answered it. */
async function pickLanguage(scope, lang) {
  const choice = scope.locator(`#language-gate [data-lang="${lang}"]`);
  if (await choice.count()) await choice.click();
}

const ROOT = path.resolve(__dirname, '../../..');
const OUT = path.resolve(process.argv[2] || './out');
const NARRATION = new Map(JSON.parse(fs.readFileSync(process.argv[3] || path.join(OUT, 'narration.json'), 'utf8')).lines.map(l => [l.id, l]));
const FILE = path.resolve(process.env.TTX_STANDALONE_FILE || path.join(ROOT, 'web/standalone/ttx.html'));
const DRY = process.env.TTX_DRY === '1';
const sleep = ms => new Promise(r => setTimeout(r, ms));
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);

function apiKey() {
  if (process.env.TTX_AI === '0') return '';
  const m = fs.readFileSync(path.join(ROOT, '.env'), 'utf8').match(/^OPENROUTER_API_KEY=(.*)$/m);
  return m ? m[1].trim() : '';
}

// A mouse pointer drawn in the page (headless Chrome renders none), following real mouse events.
const POINTER = `
(() => {
  const dot = document.createElement('div');
  dot.style.cssText = 'position:fixed;z-index:2147483647;width:22px;height:22px;margin:-4px 0 0 -4px;pointer-events:none;' +
    'background:url("data:image/svg+xml;utf8,<svg xmlns=%27http://www.w3.org/2000/svg%27 width=%2722%27 height=%2722%27 viewBox=%270 0 22 22%27><path d=%27M3 2l14 8-6 1.5L8.5 18z%27 fill=%27black%27 stroke=%27white%27 stroke-width=%271.6%27 stroke-linejoin=%27round%27/></svg>") no-repeat;left:-50px;top:-50px;transition:none';
  const add = () => document.documentElement.appendChild(dot);
  document.documentElement ? add() : addEventListener('DOMContentLoaded', add);
  addEventListener('mousemove', e => { dot.style.left = e.clientX + 'px'; dot.style.top = e.clientY + 'px'; }, true);
  addEventListener('mousedown', () => { dot.style.transform = 'scale(.82)'; }, true);
  addEventListener('mouseup', () => { dot.style.transform = ''; }, true);
})();`;

(async () => {
  fs.mkdirSync(OUT, { recursive: true });
  if (DRY) fs.mkdirSync(path.join(OUT, 'dry'), { recursive: true });
  const browser = await chromium.launch({
    channel: 'chrome', headless: true,
    args: ['--autoplay-policy=no-user-gesture-required', '--disable-background-timer-throttling', '--disable-renderer-backgrounding', '--disable-backgrounding-occluded-windows'],
  });
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1, acceptDownloads: true });
  await context.addInitScript(POINTER);
  const page = await context.newPage();
  page.on('pageerror', e => { if (!/control flow/.test(e.message)) log('PAGEERROR', e.message.slice(0, 200)); });
  await page.goto('file://' + FILE); await launch(page);

  const M = page.locator('[data-panel="master"]');
  const P = role => page.locator(`[data-panel="${role}"]`);
  const tab = role => page.locator(`.role-tabs [data-role="${role}"]`);
  await M.locator('#start').waitFor({ timeout: 60000 });

  // ---------------------------------------------------------------- setup (not recorded)
  const key = apiKey();
  if (key) {
    await page.locator('.standalone-settings summary').click();
    await page.locator('#openrouter-key').fill(key);
    await page.locator('#configure-ai').click();
    await page.locator('.standalone-settings summary').click();
    log('AI configured');
  } else log('AI off');
  await page.addStyleTag({ content: '.notification-toasts{display:none!important}' });
  await page.getByLabel('Simulazione', { exact: true }).check();
  // Give the simulator more room: the 3D view is the point of the standalone version.
  await page.getByLabel('Spazio simulazione').evaluate(el => {
    const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
    set.call(el, '44'); el.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await sleep(6000);                          // let the scene load
  await page.mouse.move(1200, 600);

  // ---------------------------------------------------------------- recorder
  const marks = [];
  let ff = null, cdp = null, last = null, sent = 0, t0 = null, ticker = null;
  if (!DRY) {
    cdp = await context.newCDPSession(page);
    ff = spawn('ffmpeg', ['-loglevel', 'error', '-y', '-f', 'image2pipe', '-c:v', 'mjpeg', '-framerate', '30', '-i', '-',
      '-c:v', 'h264_videotoolbox', '-b:v', '14M', '-pix_fmt', 'yuv420p', '-r', '30', path.join(OUT, 'screen.mkv')], { stdio: ['pipe', 'inherit', 'inherit'] });
    cdp.on('Page.screencastFrame', f => {
      last = Buffer.from(f.data, 'base64');
      cdp.send('Page.screencastFrameAck', { sessionId: f.sessionId }).catch(() => {});
      if (t0 === null) { t0 = Date.now(); fs.writeFileSync(path.join(OUT, 'video_start_ms.txt'), String(t0)); }
    });
    await cdp.send('Page.startScreencast', { format: 'jpeg', quality: 90, everyNthFrame: 1 });
    // Constant 30 fps from the wall clock: late ticks repeat the latest frame, so the timeline never drifts.
    ticker = setInterval(() => {
      if (!last) return;
      const due = Math.floor((Date.now() - t0) * 30 / 1000) + 1;
      while (sent < due) { ff.stdin.write(last); sent++; }
    }, 10);
    while (t0 === null) { await page.mouse.move(1200 + Math.random() * 4, 600); await sleep(50); }
  } else t0 = Date.now();

  let speechEnds = 0, shots = 0, waitEnd = 0; const speedups = [];
  const caption = async (id, hold = 0) => {
    const line = NARRATION.get(id);
    if (!line) throw new Error('no narration line ' + id);
    if (!DRY) { const wait = speechEnds - Date.now(); if (wait > 0) await sleep(wait); }
    const t = Date.now() - t0;
    log('SAY', id, line.duration_s ? line.duration_s.toFixed(1) + 's' : '');
    marks.push({ id, t, duration_s: line.duration_s || 0, text: line.text });
    speechEnds = Date.now() + (line.duration_s || 0) * 1000 + 450;
    if (DRY) await page.screenshot({ path: path.join(OUT, 'dry', `${String(++shots).padStart(2, '0')}_${id}.png`) });
    if (hold) await sleep(hold);
  };
  // Pause: the voice keeps going while we show something (DRY: short).
  const beat = ms => sleep(DRY ? Math.min(ms, 300) : ms);
  const shot = async name => { if (DRY) await page.screenshot({ path: path.join(OUT, 'dry', `${String(++shots).padStart(2, '0')}_${name}.png`) }); };

  // Human-like pointer: glide to the element, then click.
  const glide = async (x, y, steps = 18) => { await page.mouse.move(x, y, { steps: DRY ? 2 : steps }); };
  const click = async (loc, { pause = 350 } = {}) => {
    const el = loc.first();
    await el.scrollIntoViewIfNeeded();
    const b = await el.boundingBox();
    await glide(b.x + b.width / 2, b.y + b.height / 2);
    await beat(180);
    await el.click({ timeout: 10000 });
    await beat(pause);
  };
  const mapClick = async (canvas, fx, fy) => {
    const b = await canvas.boundingBox();
    await glide(b.x + b.width * fx, b.y + b.height * fy, 24);
    await beat(250);
    await page.mouse.down(); await page.mouse.up();
    await beat(500);
  };
  const type = async (loc, text, delay = 28) => { await click(loc, { pause: 150 }); await loc.pressSequentially(text, { delay: DRY ? 0 : delay }); };
  const closeToasts = async () => { for (const x of await page.locator('[role=status] button, .toast button, [aria-label="Chiudi"]').all().catch(() => [])) await x.click().catch(() => {}); };

  const sendInject = async title => {
    await click(M.locator(`.planned-open[aria-label*="${title}"]`));
    const dlg = page.getByRole('dialog');
    await beat(1300);
    await click(dlg.getByRole('button', { name: /^Invia a/ }), { pause: 600 });
    await dlg.waitFor({ state: 'hidden' });
  };
  const approveAll = async () => {
    await M.locator('[data-view="risposte"]').click();
    await beat(400);
    for (let i = 0; i < 12; i++) {
      const btn = M.locator('[data-proposal] [data-approve]:visible').first();
      if (!(await btn.count())) break;
      await click(btn, { pause: 700 });
    }
  };
  // The pre-advance review opens when something is unsent or needs a decision.
  const confirmAdvance = async () => {
    const ok = M.locator('[data-due-confirm]');
    if (await ok.waitFor({ timeout: 1500 }).then(() => true, () => false)) await click(ok);
  };
  const resolveTurn = async next => {
    await click(M.locator('#resolve'));
    await confirmAdvance();
    await M.locator('#turn').filter({ hasText: next }).waitFor({ timeout: 180000 });
  };

  try {
    // ------------------------------------------------------------ intro
    await caption('intro', 500);
    await page.keyboard.press('a');                   // 3D director camera: follows fire and cues
    await beat(5000);
    await caption('scenario');
    await beat(6000);

    // ------------------------------------------------------------ EXCON tabs
    await caption('tabs');
    for (const [view, wait] of [['risposte', 3500], ['map', 4500], ['history', 3500], ['timeline', 500]]) {
      await click(M.locator(`[data-view="${view}"]`), { pause: 0 });
      await beat(wait);
    }
    await caption('start');
    await click(M.locator('#start'), { pause: 1500 });

    // ------------------------------------------------------------ turn 1: injects
    await caption('injects');
    await sendInject('Fire reported on Pine Hill');
    await sendInject('School in session');
    await sendInject('Fire behaviour estimate');
    await closeToasts();

    // ------------------------------------------------------------ mayor
    await click(tab('mayor'), { pause: 400 });
    await pickLanguage(P('mayor'), 'it');
    await caption('mayor_role');
    await beat(5000);
    await click(P('mayor').locator('[data-view="map"]'));
    await caption('mayor_inject');
    await beat(5500);

    const W = P('mayor').locator('.work-modal');
    const mayorEvacuation = async (captionId, followUp) => {
      await click(P('mayor').locator('#new-player-action'));
      await W.waitFor();
      if (captionId) await caption(captionId);
      // The narrative alone is enough: the AI reads it and suggests actions to add.
      await type(W.locator('#response-narrative'), 'Evacuare la scuola primaria di Pineta verso il centro sportivo di Valverde e avvisare la popolazione di Pineta.');
      let ai = false;
      if (key) {
        const add = W.locator('[data-add-suggestions], [data-add-suggestion="0"]').first();
        let quiet = null;
        for (let i = 0; i < 90 && !ai; i++) {
          await sleep(500); ai = await add.isEnabled().catch(() => false);
          if (quiet === null && Date.now() > speechEnds) quiet = Date.now() - t0;
        }
        // The wait for the model after the voice has finished is shown sped up (compose.py).
        if (!DRY && quiet !== null && Date.now() - t0 - quiet > 2500) speedups.push({ from: quiet + 600, to: Date.now() - t0 - 400, factor: 8 });
        log('AI suggestions usable:', ai);
        if (ai) { waitEnd = Date.now(); if (followUp) await caption(followUp); await beat(1500); await shot('ai_draft'); await click(add, { pause: 900 }); await click(W.locator('[data-respond]'), { pause: 900 }); }
      }
      if (!ai) {
        // Fallback: the structured form (same command the AI would have produced).
        await W.locator('#work-action').selectOption('EvacuateBuilding');
        await W.locator('#work-location').selectOption({ label: 'Pineta Primary School' });
        if (followUp) await caption(followUp);
        await click(W.locator('[data-add-action]'), { pause: 500 });
        await click(W.locator('[data-respond]'), { pause: 900 });
      }
      await W.waitFor({ state: 'hidden' });
    };
    await mayorEvacuation('mayor_ai', 'ai_draft');

    // ------------------------------------------------------------ fire commander
    await click(tab('fire_commander'), { pause: 400 });
    await click(P('fire_commander').locator('[data-view="map"]'));
    await caption('fire_action');
    await click(P('fire_commander').locator('#new-player-action'));
    const F = P('fire_commander').locator('.work-modal');
    await F.waitFor();
    await F.locator('#work-action').selectOption('CutFireLine');
    await beat(600);
    const fmap = F.locator('canvas#work-map');
    await click(F.getByRole('button', { name: /Primo punto|First point/ }), { pause: 200 });
    await mapClick(fmap, 0.60, 0.40);
    await click(F.getByRole('button', { name: /Secondo punto|Second point/ }), { pause: 200 });
    await mapClick(fmap, 0.80, 0.40);
    await beat(800);
    await click(F.locator('[data-add-action]'), { pause: 500 });
    await click(F.locator('[data-respond]'), { pause: 900 });
    await F.waitFor({ state: 'hidden' });
    await click(P('fire_commander').locator('#new-player-action'));
    await F.waitFor();
    await F.locator('#work-action').selectOption('WaterDrop');
    await mapClick(F.locator('canvas#work-map'), 0.80, 0.31);
    await click(F.locator('[data-add-action]'), { pause: 500 });
    await click(F.locator('[data-respond]'), { pause: 900 });
    await F.waitFor({ state: 'hidden' });

    // ------------------------------------------------------------ chat
    await caption('chat');
    await click(P('fire_commander').locator('[data-view="chat"]'));
    await click(P('fire_commander').locator('[data-contact="mayor"]'));
    await type(P('fire_commander').locator('#text'), 'Mayor, we are cutting a fire line north of Pineta. Keep the north road clear for my crews.');
    await click(P('fire_commander').locator('#send'), { pause: 1000 });
    await click(tab('mayor'));
    await click(P('mayor').locator('[data-view="chat"]'));
    await click(P('mayor').locator('[data-contact="fire_commander"]'));
    await beat(5000);

    // ------------------------------------------------------------ regional CP: extra bus
    await caption('bus_request');
    await click(tab('regional_cp'));
    await pickLanguage(P('regional_cp'), 'hy');
    await click(P('regional_cp').locator('[data-view="map"]'));
    await click(P('regional_cp').locator('#new-player-action'));
    const R = P('regional_cp').locator('.work-modal');
    await R.waitFor();
    await R.locator('#work-action').selectOption('RequestReinforcement');
    await beat(400);
    await R.locator('#work-resource').selectOption('buses');
    await R.locator('#work-qty').fill('1');
    await beat(1200);
    await click(R.locator('[data-add-action]'), { pause: 500 });
    await click(R.locator('[data-respond]'), { pause: 900 });
    await R.waitFor({ state: 'hidden' });

    // ------------------------------------------------------------ EXCON review
    const dlgx = page.locator('.response-dialog');
    const openRow = async filter => { await click(M.locator('.response-row').filter(filter).first(), { pause: 900 }); };
    const closeDlg = async () => { if (await dlgx.count()) await click(dlgx.getByRole('button', { name: 'Chiudi' }), { pause: 500 }); };
    const approveInDialog = async () => {
      for (let i = 0; i < 4; i++) {
        const b = dlgx.locator('[data-approve]:not([disabled])').first();
        if (!(await b.count())) break;
        await click(b, { pause: 1200 });
      }
    };
    await click(tab('master'));
    await click(M.locator('[data-view="risposte"]'));
    await caption('review');
    await openRow({ hasText: 'School in session' });
    await beat(5000);
    await caption('bus');
    await beat(2500);
    await caption('reject');
    await type(dlgx.locator('[data-note]'), 'Servono 3 autobus, ne avete 2: il terzo arriva alle 13:30. Riproponi l\'evacuazione nel prossimo turno.', 22);
    await beat(800);
    await click(dlgx.locator('[data-reject]'), { pause: 1200 });
    await closeDlg().catch(() => {});
    await caption('approve');
    for (let i = 0; i < 6; i++) {
      const pending = M.locator('.response-row.is-pending');
      if (!(await pending.count())) break;
      await click(pending.first(), { pause: 900 });
      await approveInDialog();
      await closeDlg();
    }

    // ------------------------------------------------------------ speed + resolve turn 1
    await caption('speed');
    await click(M.locator('[data-view="map"]'));
    await click(M.locator('[data-tool="speed"]'));
    const tool = page.getByRole('dialog');
    await tool.locator('input').first().fill('60');
    await beat(1200);
    await click(tool.getByRole('button', { name: /Invia|Applica/ }).first(), { pause: 1000 });
    await shot('speed_set');
    await tool.getByRole('button', { name: 'Chiudi' }).click().catch(() => {});
    await caption('resolve1');
    await click(M.locator('#resolve'));
    await confirmAdvance();
    await M.locator('#turn').filter({ hasText: 'Turno 2' }).waitFor({ timeout: 240000 });
    await beat(2000);
    await caption('turn1_end');
    await click(tab('mayor'));
    await click(P('mayor').locator('[data-view="function"]'));
    await P('mayor').locator('h2', { hasText: 'Riepiloghi' }).scrollIntoViewIfNeeded();
    await beat(6000);

    // ------------------------------------------------------------ turn 2
    await click(tab('master'));
    await click(M.locator('[data-view="timeline"]'));
    await caption('turn2');
    await sendInject('Wind strengthening');
    await sendInject('Northern Road congestion');
    await click(tab('mayor'));
    await click(P('mayor').locator('[data-view="map"]'));
    await mayorEvacuation('mayor_retry', null);
    await caption('regional');
    await click(tab('regional_cp'));
    await click(P('regional_cp').locator('[data-view="map"]'));
    await click(P('regional_cp').locator('#new-player-action'));
    await R.waitFor();
    await R.locator('#work-action').selectOption('RequestReinforcement');
    await R.locator('#work-resource').selectOption('ground_crews');
    await R.locator('#work-qty').fill('2');
    await beat(1200);
    await click(R.locator('[data-add-action]'), { pause: 500 });
    await click(R.locator('[data-respond]'), { pause: 900 });
    await R.waitFor({ state: 'hidden' });
    await click(tab('master'));
    await click(M.locator('[data-view="risposte"]'));
    await caption('approve2');
    for (let i = 0; i < 6; i++) {
      const pending = M.locator('.response-row.is-pending');
      if (!(await pending.count())) break;
      await click(pending.first(), { pause: 900 });
      await approveInDialog();
      await closeDlg();
    }
    await click(M.locator('#resolve'));
    await confirmAdvance();
    await M.locator('#turn').filter({ hasText: 'Turno 3' }).waitFor({ timeout: 240000 });
    await beat(1500);
    await caption('history');
    await click(M.locator('[data-view="history"]'));
    await beat(9000);
    await sleep(DRY ? 0 : Math.max(0, speechEnds - Date.now()));
    await beat(2500);
    throw new Error('DONE');
  } catch (e) {
    if (e.message !== 'DONE') {
      log('FAILED', e.message);
      await page.screenshot({ path: path.join(OUT, 'failure.png') }).catch(() => {});
    }
    var failed = e.message !== 'DONE';
  }
  if (!DRY) {
    await sleep(1500);
    clearInterval(ticker);
    await cdp.send('Page.stopScreencast').catch(() => {});
    await new Promise(r => { ff.on('close', r); ff.stdin.end(); });
    log(`screen.mkv: ${sent} frames, ${(sent / 30).toFixed(1)} s`);
  }
  fs.writeFileSync(path.join(OUT, 'marks.json'), JSON.stringify({ marks, speedups }, null, 1));
  await browser.close();
  process.exit(failed ? 1 : 0);
})();

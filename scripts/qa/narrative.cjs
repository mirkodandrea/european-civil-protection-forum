// Invisible browser QA through real HTTP commands and player/EXCON controls.
// Uses a separate headless server, no saved session and no external model.
const { chromium } = require(process.env.TTX_PLAYWRIGHT || '../record/node_modules/playwright');
const assert = require('assert/strict');
const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');
const root = path.resolve(__dirname, '../..');
const out = path.resolve(process.env.TTX_QA_OUT || path.join(root, 'runs/narrative-review'));
const base = process.env.TTX_QA_URL || 'http://127.0.0.1:8097';
const bin = process.env.TTX_QA_BIN || path.join(root, 'target/release/ttx');
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
(async () => {
  fs.mkdirSync(out, { recursive: true });
  let log = '', browser;
  const child = spawn(bin, ['--headless', '--scenario', 'data/scenarios/synthetic_valley.json', '--port', new URL(base).port, '--web', process.env.TTX_QA_WEB || 'web/dist', '--no-state'], { cwd: root, env: { ...process.env, OPENROUTER_API_KEY: '' } });
  child.stdout.on('data', b => { log += b; }); child.stderr.on('data', b => { log += b; });
  const api = async (who, route, body) => {
    const response = await fetch(base + route, { method: body === undefined ? 'GET' : 'POST', headers: { Authorization: `Bearer ${who}`, 'Content-Type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) });
    const data = await response.json(); assert.equal(response.status, 200, JSON.stringify(data)); return data;
  };
  try {
    let ready = false;
    for (let i = 0; i < 100; i++) { try { await api('master', '/api/state'); ready = true; break; } catch { if (child.exitCode !== null) throw Error(log); await wait(200); } }
    assert(ready, log);
    await api('master', '/api/master/start', { force: true });
    await api('master', '/api/player/mayor/language', { language: 'en' });
    await api('master', '/api/master/inject', { title: 'Support for evacuated families', sender: '112', recipients: ['mayor'], text: 'Families at the shelter request psychological support. Coordinate assistance.' });
    const before = (await api('master', '/api/state')).world;
    browser = await chromium.launch({ headless: true, channel: 'chrome' });
    const errors = [];
    const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
    const player = await context.newPage(); player.on('pageerror', e => errors.push(e.message));
    await context.route('https://openrouter.ai/**', route => { errors.push('Unexpected external AI request'); return route.abort(); });
    await player.goto(base + '/player.html?role=mayor');
    // Desktop-only flow: the inject drill-down opens the Response modal ("Rispondi").
    await player.locator('[data-view="map"]').click();
    await player.locator('[data-inject-row]').filter({ hasText: 'Support for evacuated families' }).click();
    await player.locator('[data-reply-inject]').click();
    const work = player.locator('.work-modal');
    await work.locator('#response-narrative').fill('Activate psychological support for families at the shelter.');
    await player.screenshot({ path: path.join(out, 'player-narrative-desktop.png') });
    await work.locator('[data-respond]').click();
    await work.waitFor({ state: 'hidden' });
    let proposals;
    for (let i = 0; i < 50; i++) { proposals = (await api('master', '/api/master')).proposals; if (proposals.length) break; await wait(100); }
    assert.equal(proposals.length, 1); const id = proposals[0].id;
    assert.equal(proposals[0].narrative, true); assert.equal(proposals[0].status, 'pending'); assert.deepEqual(proposals[0].commands, []);
    const master = await context.newPage(); master.on('pageerror', e => errors.push(e.message));
    await master.goto(base + '/master.html');
    await master.locator('[data-view="risposte"]').click();
    await master.locator('.response-row').filter({ hasText: 'Support for evacuated families' }).click();
    let card = master.locator(`[data-proposal="${id}"]`);
    await card.locator('[data-approve]').click();
    await card.locator('[data-narrative-outcome]').waitFor();
    await card.locator('[data-narrative-outcome]').fill('A support team is available at the shelter.');
    // EXCON speaks as a persona and may add an intervention (JSON tool: no map pick needed here).
    await card.locator('[data-persona]').selectOption({ label: 'Mayor of Valverde' }).catch(() => card.locator('[data-persona]').selectOption({ index: 1 }));
    await card.locator('.staged-commands summary').click();
    await card.locator('.staged-commands textarea').fill('{"type":"IssueWarning","zone":"F4"}');
    await card.getByRole('button', { name: 'Aggiungi comando' }).click();
    await card.locator('[data-staged-command]').waitFor();
    await master.screenshot({ path: path.join(out, 'excon-approved-narrative.png') });
    await card.locator('[data-record-narrative-outcome]').click();
    await card.locator('.narrative-outcome').filter({ hasText: 'support team is available' }).waitFor();
    await card.locator('[data-outcome-intervention]').waitFor();
    await master.screenshot({ path: path.join(out, 'excon-recorded-outcome.png') });
    const after = (await api('master', '/api/master')).proposals.find(p => p.id === id);
    assert.equal(after.status, 'approved'); assert.equal(after.narrative_outcomes.length, 1); assert.deepEqual(after.executions, []);
    assert.equal(after.narrative_outcomes[0].interventions.length, 1, 'outcome linked to its EXCON intervention');
    await player.reload();
    await player.locator('[data-view="map"]').click();
    await player.locator('[data-inject-row]').filter({ hasText: 'Support for evacuated families' }).click();
    await player.locator('[data-inject-detail]').getByText(/A support team is available at the shelter/).waitFor();
    assert(!(await player.locator('body').innerText()).includes('EXCON'), 'players never see EXCON');
    await player.screenshot({ path: path.join(out, 'player-recorded-outcome.png') });
    const other = await api('fire_commander', '/api/player/fire_commander'); assert(!JSON.stringify(other).includes('psychological support for families'));
    const debrief = await api('master', '/api/debrief'); assert(JSON.stringify(debrief).includes('narrative_outcomes'));
    assert.deepEqual(errors, []);
    fs.writeFileSync(path.join(out, 'results.json'), JSON.stringify({ passed: true, proposal: id, checks: ['response modal', 'persona', 'outcome with intervention', 'approval', 'EXCON outcome', 'player thread after reload', 'role isolation', 'debrief', 'no physical changes', 'no external AI'], errors }, null, 2));
    console.log(`Narrative browser QA passed: ${out}`);
  } finally {
    if (browser) await browser.close(); child.kill(); fs.writeFileSync(path.join(out, 'server.log'), log);
  }
})().catch(e => { console.error(e); process.exitCode = 1; });

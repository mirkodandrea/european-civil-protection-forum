// Invisible standalone QA of a narrated decision: the player answers an inject
// with Rispondi (decision, basis), EXCON approves it in Risposte and records the
// outcome as a persona with an intervention; approval and outcome are distinct
// states. Runs on the standalone app (TTX_QA_URL, e.g. a Trunk server; default
// the single-file build); no external model.
const { chromium } = require(process.env.TTX_PLAYWRIGHT || '../record/node_modules/playwright');
const assert = require('assert/strict');
const fs = require('fs');
const path = require('path');
const root = path.resolve(__dirname, '../..');
const out = path.resolve(process.env.TTX_QA_OUT || path.join(root, 'runs/narrative-review'));
const url = process.env.TTX_QA_URL || ('file://' + path.join(root, process.env.TTX_STANDALONE_FILE || 'web/standalone/ttx.html'));
(async () => {
  fs.mkdirSync(out, { recursive: true });
  const browser = await chromium.launch({ headless: true, channel: process.env.TTX_BROWSER_CHANNEL || 'chrome' });
  const errors = [];
  try {
    const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
    page.on('pageerror', e => { if (!e.message.startsWith('Using exceptions for control flow')) errors.push(e.message); });
    await page.route('https://openrouter.ai/**', route => { errors.push('Unexpected external AI request'); return route.abort(); });
    const master = page.locator('[data-panel="master"]'), mayor = page.locator('[data-panel="mayor"]');
    const choose = role => page.locator(`.role-tabs [data-role="${role}"]`).click();
    const lang = async (p, l) => { const c = p.locator(`#language-gate [data-lang="${l}"]`); if (await c.count()) await c.click(); };
    await page.goto(url);
    if (url.startsWith('http')) { await page.evaluate(() => { localStorage.clear(); sessionStorage.clear(); }); await page.goto(url); }
    await page.locator('.launcher #start-session').click();
    await require('./standalone-languages.cjs').confirmLanguages(page);
    await master.locator('#start').click();
    await master.locator('#turn').filter({ hasText: /orders|ordini/ }).waitFor();
    // EXCON sends a custom inject to the mayor only.
    await master.locator('[data-view="timeline"]').click();
    await master.locator('[data-inject-all]').click();
    const modal = page.getByRole('dialog');
    await modal.locator('#inject-title').fill('Support for evacuated families');
    await modal.locator('#inject-sender').fill('112');
    await modal.locator('#text').fill('Families at the shelter request psychological support. Coordinate assistance.');
    await modal.locator('#inject-scope').selectOption('custom');
    await modal.getByLabel(/Sindaco|Mayor/).check();
    await modal.locator('#send').click();
    await modal.waitFor({ state: 'hidden' });

    // Desktop flow: the inject drill-down opens the Rispondi modal.
    await choose('mayor'); await lang(mayor, 'en');
    await mayor.locator('[data-view="map"]').click();
    await mayor.locator('[data-inject-row]').filter({ hasText: 'Support for evacuated families' }).click();
    await mayor.locator('[data-reply-inject]').click();
    const work = mayor.locator('.work-modal');
    await work.locator('#response-narrative').fill('Activate psychological support for families at the shelter.');
    // A narrated decision: explicit kind and its basis are required.
    await work.locator('#kind-decision').check();
    assert(!(await work.locator('[data-respond]').isEnabled()), 'no basis, no send');
    await work.locator('#basis-operational_judgment').check();
    await page.screenshot({ path: path.join(out, 'player-narrative-desktop.png') });
    await work.locator('[data-respond]').click();
    await work.waitFor({ state: 'hidden' });
    await mayor.locator('[data-response-state="rs_review"]').waitFor();

    // EXCON approves in Risposte, then records the outcome (a distinct event).
    await choose('master');
    await master.locator('[data-view="risposte"]').click();
    await master.locator('.response-row').filter({ hasText: 'Support for evacuated families' }).click();
    const dialog = master.locator('.response-dialog');
    await dialog.locator('[data-response-state="rs_review"]').waitFor();
    let card = dialog.locator('[data-proposal]').first();
    await card.locator('[data-approve]').click();
    await dialog.locator('[data-response-state="rs_approved"]').waitFor();
    card = dialog.locator('[data-proposal]').first();
    await card.locator('[data-narrative-outcome]').waitFor();
    await card.locator('[data-narrative-outcome]').fill('A support team is available at the shelter.');
    // EXCON speaks as a persona and may add an intervention (JSON tool: no map pick needed here).
    await card.locator('[data-persona]').selectOption({ index: 1 });
    await card.locator('.staged-commands summary').click();
    await card.locator('.staged-commands textarea').fill('{"type":"IssueWarning","zone":"F4"}');
    await card.getByRole('button', { name: /Aggiungi comando|Add command/ }).click();
    await card.locator('[data-staged-command]').waitFor();
    await page.screenshot({ path: path.join(out, 'excon-approved-narrative.png') });
    await card.locator('[data-record-narrative-outcome]').click();
    await card.locator('.narrative-outcome').filter({ hasText: 'support team is available' }).waitFor();
    await card.locator('[data-outcome-intervention]').waitFor();
    await dialog.locator('[data-response-state="rs_outcome"]').waitFor();
    await page.screenshot({ path: path.join(out, 'excon-recorded-outcome.png') });
    await dialog.getByRole('button', { name: /^(Chiudi|Close)$/ }).first().click();

    // The player reads the outcome in the inject's history; nobody else sees it.
    await choose('mayor');
    await mayor.locator('[data-inject-detail]').getByText(/A support team is available at the shelter/).waitFor();
    await mayor.locator('[data-response-state="rs_outcome"]').waitFor();
    assert(!(await mayor.innerText()).includes('EXCON'), 'players never see EXCON');
    await page.screenshot({ path: path.join(out, 'player-recorded-outcome.png') });
    await choose('fire_commander'); await lang(page.locator('[data-panel="fire_commander"]'), 'en');
    await page.locator('[data-panel="fire_commander"] [data-view="map"]').click();
    assert(!(await page.locator('[data-panel="fire_commander"]').innerText()).includes('Support for evacuated families'), 'role isolation');

    // The debrief keeps the outcome and its intervention; the player executed nothing.
    await choose('master'); await master.locator('[data-view="map"]').click(); await master.locator('[data-tool="end"]').click();
    const pending = page.waitForEvent('download'); await master.locator('#debriefdownload').click();
    const file = path.join(out, 'debrief.json'); await (await pending).saveAs(file);
    const debrief = JSON.parse(fs.readFileSync(file, 'utf8'));
    const p = debrief.proposals.find(x => x.narrative);
    assert.equal(p.narrative_outcomes.length, 1);
    assert.equal(p.narrative_outcomes[0].interventions.length, 1, 'outcome linked to its EXCON intervention');
    assert.deepEqual(p.executions, []);
    assert.deepEqual(errors, []);
    fs.writeFileSync(path.join(out, 'results.json'), JSON.stringify({ passed: true, url, checks: ['Rispondi modal', 'basis required', 'approval', 'persona', 'outcome with intervention', 'distinct states', 'player history', 'role isolation', 'debrief', 'no external AI'], errors }, null, 2));
    console.log(`Narrative browser QA passed: ${out}`);
  } finally {
    await browser.close();
  }
})().catch(e => { console.error(e); process.exitCode = 1; });

// Invisible QA for the read-only player Function view on the live standalone app.
// OpenRouter is intercepted in-browser; no external model request is sent.
const { chromium } = require(process.env.TTX_PLAYWRIGHT || '../record/node_modules/playwright');
const fs = require('fs');
const path = require('path');
const assert = require('assert/strict');

/** The start-of-session language gate; a no-op when this session already answered it. */
async function pickLanguage(scope, lang) {
  const choice = scope.locator(`#language-gate [data-lang="${lang}"]`);
  if (await choice.count()) await choice.click();
}

const cwd = path.resolve(__dirname, '../..');
const out = path.resolve(process.env.TTX_QA_OUT || path.join(cwd, 'runs/player-function-review'));
const base = process.env.TTX_QA_URL || 'http://127.0.0.1:8080';

(async () => {
  fs.mkdirSync(out, { recursive: true });
  const browser = await chromium.launch({ headless: true, channel: 'chrome' });
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 }, deviceScaleFactor: 1 });
  const errors = [], mockedTasks = [], runtimeSignals = [];
  const privacyChecks = [];
  page.on('pageerror', error => {
    if (error.message === "Using exceptions for control flow, don't mind me. This isn't actually an error!") runtimeSignals.push('winit event-loop control flow');
    else errors.push(error.message);
  });
  await page.route('https://openrouter.ai/api/v1/chat/completions', async route => {
    const request = route.request().postDataJSON();
    const system = request.messages?.[0]?.content || '';
    let answer;
    if (system.includes('private end-of-turn recap')) {
      mockedTasks.push('player_turn_summary');
      const evidence = JSON.parse(request.messages.at(-1).content);
      if (evidence.role === 'mayor') {
        assert(!JSON.stringify(evidence).includes('Coordinate access control'), 'private fire inject never enters mayor AI context');
        privacyChecks.push('mayor AI context excludes fire private inject');
      }
      answer = 'Turno 1: nessuna proposta operativa inviata; risorse comunali disponibili: 2 autobus e 2 pattuglie.';
    } else {
      mockedTasks.push('other');
      answer = request.messages?.at(-1)?.content || '';
    }
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      headers: { 'Access-Control-Allow-Origin': '*' },
      body: JSON.stringify({ choices: [{ message: { content: answer } }] }),
    });
  });

  const master = page.locator('[data-panel="master"]');
  const mayor = page.locator('[data-panel="mayor"]');
  const choose = async role => page.locator(`.role-tabs [data-role="${role}"]`).click();
  try {
    await page.goto(base, { waitUntil: 'domcontentloaded' });
    await master.locator('#start').waitFor({ timeout: 30000 });

    // Configure the browser LLM with a fixture credential intercepted above.
    await page.locator('.standalone-settings summary').click();
    await page.locator('#openrouter-key').fill('qa-fixture-only');
    await page.locator('#configure-ai').click();
    assert.equal(await page.locator('#openrouter-key').inputValue(), '', 'fixture credential is cleared after configuration');

    await page.locator('.standalone-settings summary').click();
    await page.getByLabel('Simulazione', {exact:true}).uncheck();
    await master.locator('#start').click();
    await master.locator('#turn').filter({ hasText: 'orders' }).waitFor();

    // Send a private, real EXCON inject to the fire commander.
    await master.locator('[data-view="timeline"]').click();
    await master.locator('[data-inject-role="fire_commander"]').click();
    let modal = page.getByRole('dialog');
    await page.waitForFunction(() => document.activeElement?.id === 'inject-title');
    await modal.locator('#inject-title').fill('Patrol coordination');
    await modal.locator('#text').fill('Coordinate access control with the municipal police.');
    await modal.locator('#send').click();
    await modal.waitFor({ state: 'hidden' });

    await choose('mayor');
    await pickLanguage(mayor, 'it');
    await mayor.locator('[data-view="function"]').click();
    const functionView = mayor.locator('#function-view');
    await functionView.waitFor();
    for (const resourceName of ['Buses', 'Police patrols']) {
      const card = functionView.locator('.function-resource').filter({ has: page.locator('h3', { hasText: resourceName }) });
      await card.waitFor();
      assert.equal((await card.locator('dl div').nth(0).locator('dd').innerText()).trim(), '2',
        `${resourceName} shows two available units`);
    }
    assert(await functionView.getByText('50 seats each; needed to evacuate schools and hospitals.', { exact: false }).count() > 0,
      'resource description includes bus capacity');
    assert(await functionView.getByText('Each patrol can close one road to civilian traffic per turn.', { exact: false }).count() > 0,
      'resource description includes patrol capability');
    assert.equal(await functionView.locator('input, button, select, textarea, form').count(), 0,
      'Function view is read-only');
    assert(await mayor.locator('[data-view="map"]').count() > 0, 'Injects navigation exists');
    assert(await mayor.locator('[data-view="chat"]').count() > 0, 'Conversations navigation exists');
    await page.screenshot({ path: path.join(out, 'mayor-function-desktop.png'), fullPage: true });

    await page.setViewportSize({ width: 390, height: 844 });
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false,
      '390px mobile viewport has no horizontal overflow');
    await page.screenshot({ path: path.join(out, 'mayor-function-mobile.png'), fullPage: true });
    await functionView.locator('.function-resource').first().scrollIntoViewIfNeeded();
    await page.screenshot({ path: path.join(out, 'mayor-function-mobile-assets.png') });

    // Resolve one simulated minute through EXCON controls; summary comes through the mocked model.
    await choose('master');
    await master.locator('[data-view="timeline"]').click();
    await master.locator('#minutes').fill('1');
    await master.locator('#resolve').click();
    await master.locator('#turn').filter({ hasText: 'Turno 2 · orders' }).waitFor({ timeout: 30000 });

    await choose('mayor');
    await mayor.locator('[data-view="function"]').click();
    await functionView.getByText('Turno 1: nessuna proposta operativa inviata', { exact: false }).waitFor({ timeout: 30000 });
    assert(mockedTasks.includes('player_turn_summary'), 'player summary was generated via mocked OpenRouter response');
    assert.equal(await functionView.getByText('Patrol coordination', { exact: false }).count(), 0,
      'mayor cannot see the inject delivered only to the fire commander');
    await functionView.locator('.function-summaries').scrollIntoViewIfNeeded();
    await page.screenshot({ path: path.join(out, 'mayor-function-summary.png'), fullPage: true });
    assert(privacyChecks.length > 0, 'role privacy was checked on the actual AI input');

    assert.equal(errors.length, 0, errors.join('\n'));
    fs.writeFileSync(path.join(out, 'manifest.json'), JSON.stringify({
      app: base,
      scenario: 'valverde_factory',
      flow: ['start EXCON', 'send inject privately to fire_commander', 'inspect mayor Function view', 'mobile overflow check at 390px', 'resolve one minute', 'inspect mayor turn summary'],
      mockedTasks, runtimeSignals, privacyChecks,
      screenshotFiles: ['mayor-function-desktop.png', 'mayor-function-mobile.png', 'mayor-function-mobile-assets.png', 'mayor-function-summary.png'],
      errors,
    }, null, 2));
    console.log(`PASS player Function view QA: ${out}`);
  } catch (error) {
    fs.writeFileSync(path.join(out, 'failure.json'), JSON.stringify({ error: String(error), stack: error.stack, errors, mockedTasks }, null, 2));
    try { await page.screenshot({ path: path.join(out, 'failure.png'), fullPage: true }); } catch (_) {}
    throw error;
  } finally {
    await browser.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });

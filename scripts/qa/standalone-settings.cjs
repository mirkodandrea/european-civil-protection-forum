// Invisible standalone settings fixture: no server, no live AI, no credentials.
// file:// build, or TTX_QA_URL for a Trunk server.
const {chromium} = require(process.env.TTX_PLAYWRIGHT || 'playwright');
const assert = require('assert/strict');
const fs = require('fs');
const path = require('path');

/** The launcher shown before the exercise: resume the saved session, else start the selected scenario. */
async function launch(page) {
  await page.locator('.launcher, [data-panel="master"] #turn-controls').first().waitFor({ state: 'attached', timeout: 60000 });
  const button = page.locator('.launcher #resume-session, .launcher #start-session').first();
  if (await button.count()) await button.click();
}
const storageKey = 'ttx.standalone.openrouter-key';
const token = 'fixture-key-not-a-secret';
const url = process.env.TTX_QA_URL || ('file://' + path.resolve(process.env.TTX_STANDALONE_FILE || 'web/standalone/ttx.html'));
const out = process.env.TTX_QA_OUT || path.join('runs/standalone-settings', new Date().toISOString().replace(/[:.]/g, '-'));
(async () => {
  fs.mkdirSync(out, {recursive: true});
  const browser = await chromium.launch({headless: true, ...(process.env.TTX_BROWSER_EXECUTABLE
    ? {executablePath: process.env.TTX_BROWSER_EXECUTABLE}
    : {channel: process.env.TTX_BROWSER_CHANNEL || 'chrome'}),
    args: process.platform === 'linux' ? ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] : []});
  const page = await browser.newPage({viewport: {width: 1680, height: 1100}});
  const errors = [];
  page.on('pageerror', error => {
    if (!error.message.startsWith('Using exceptions for control flow')) errors.push(error.message);
  });
  await page.addInitScript(() => {
    window.webglStarts = 0;
    const original = HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext = function(type, ...args) {
      if (type.startsWith('webgl')) window.webglStarts++;
      return original.call(this, type, ...args);
    };
  });
  // The exercise is up (a resumed session may already be past the briefing).
  const ready = () => page.locator('[data-panel="master"] #turn').waitFor({timeout: 60000});
  // The AI settings live in the EXCON "Impostazioni" view.
  const settings = page.locator('#app-settings');
  const openSettings = async () => {
    await page.locator('.role-tabs [data-role="master"]').click();
    await page.locator('[data-panel="master"] [data-view="scenario"]').click();
    await settings.waitFor();
  };
  try {
    await page.goto(url);
    if (url.startsWith('http')) { await page.evaluate(() => { localStorage.clear(); sessionStorage.clear(); }); await page.goto(url); }
    await page.locator('.launcher').waitFor({timeout: 90000});
    // Offline like the single-file app (a served build must reload from its server).
    if (!url.startsWith('http')) await page.context().setOffline(true);
    await launch(page);
    await ready();
    assert.equal(await page.getByLabel('Simulazione', {exact: true}).isChecked(), false);
    assert(await page.locator('.standalone-simulation').isHidden());
    await page.waitForTimeout(500);
    assert.equal(await page.evaluate(() => window.webglStarts), 0, 'disabled view never starts WebGL');
    await page.screenshot({path: path.join(out, 'default-disabled.png')});
    await openSettings();
    await page.locator('#openrouter-key').fill('  ' + token + '  ');
    await page.locator('#configure-ai').click();
    assert.equal(await page.evaluate(key => localStorage.getItem(key), storageKey), token);
    assert.equal(await page.locator('#openrouter-key').inputValue(), '');
    await page.reload(); await launch(page);
    await ready();
    await openSettings();
    assert.match(await settings.innerText(), /AI configurata · chiave salvata nel browser/);
    // Prove the restored token configures real browser AI, rather than just the notice.
    await page.context().setOffline(false);
    let requests = 0;
    await page.route('https://openrouter.ai/api/v1/chat/completions', async route => {
      assert.equal(route.request().headers().authorization, 'Bearer ' + token);
      requests++;
      await route.fulfill({status: 200, contentType: 'application/json', body: JSON.stringify({choices: [{message: {content: JSON.stringify({actions:[{type:'IssueWarning', zone:'F4'}], reasons:['warn F4'], summary:'Warning for F4'})}}]})});
    });
    await require('./standalone-languages.cjs').confirmLanguages(page);
    await page.locator('[data-panel="master"] [data-view="timeline"]').click();
    await page.locator('[data-panel="master"] #start').click();
    // The mayor answers an inject: EXCON sends the first planned one.
    await page.locator('[data-panel="master"] [data-send-open]').first().click();
    await page.locator('[data-panel="master"] [data-send]').click();
    await page.locator('.role-tabs [data-role="mayor"]').click();
    const mayor = page.locator('[data-panel="mayor"]');
    await mayor.locator('[data-view="map"]').click();
    await mayor.locator('[data-inject-row]').first().click();await mayor.locator('[data-reply-direct]').click();
    await mayor.locator('#response-narrative').fill('We issue a warning for F4 now.');
    await mayor.locator('[data-add-suggestion="0"]').waitFor({timeout: 15000});
    assert(requests > 0);
    await page.getByRole('dialog').locator('header button').click();
    await page.getByRole('dialog').waitFor({state: 'hidden'});
    await openSettings();
    await page.locator('#clear-ai').click();
    assert.equal(await page.evaluate(key => localStorage.getItem(key), storageKey), null);
    await page.reload(); await launch(page);
    await ready();
    await openSettings();
    // The saved key is gone. (A resumed session can still report AI from its
    // recorded answers, labelled ".env": see docs/todo.md.)
    assert.doesNotMatch(await settings.innerText(), /chiave salvata nel browser/);
    // The optional renderer still works and can be hidden and re-enabled.
    console.log('PASS token persistence, authenticated reuse and removal');
    await page.getByLabel('Simulazione', {exact: true}).check();
    await page.waitForFunction(() => window.webglStarts > 0, {timeout: 60000});
    await page.waitForTimeout(5000);
    await page.screenshot({path: path.join(out, 'simulation-low.png')});
    const starts = await page.evaluate(() => window.webglStarts);
    const canvas = await page.locator('#standalone-3d').evaluate(el => ({width: el.width, cssWidth: el.getBoundingClientRect().width}));
    await page.getByLabel('Simulazione', {exact: true}).uncheck();
    await page.getByLabel('Simulazione', {exact: true}).check();
    await page.waitForTimeout(500);
    assert.equal(await page.evaluate(() => window.webglStarts), starts, 'toggle reuses renderer');
    // Browsers that deny local storage must retain a usable manual UI.
    await page.addInitScript(() => Object.defineProperty(window, 'localStorage', {get() {throw new DOMException('Blocked', 'SecurityError');}}));
    await page.reload(); await launch(page);
    await ready();
    await openSettings();
    await page.locator('#openrouter-key').fill(token);
    await page.locator('#configure-ai').click();
    assert.match(await settings.innerText(), /Archiviazione browser non disponibile/);
    await page.locator('#clear-ai').click();
    assert.match(await settings.innerText(), /impossibile rimuovere/);
    assert.deepEqual(errors, []);
    fs.writeFileSync(path.join(out, 'manifest.json'), JSON.stringify({fixture: 'standalone settings; seed 1; 32 realizations; mocked OpenRouter', checks: ['disabled startup without WebGL', 'save and restore token', 'restored token used by AI', 'remove and reload', 'optional low-quality renderer', 'toggle renderer', 'blocked storage'], canvas, errors}, null, 2));
    console.log('PASS standalone settings:', out);
  } catch (error) {
    try { await page.screenshot({path: path.join(out, 'failure.png')}); } catch (_) {}
    throw error;
  } finally {
    await browser.close();
  }
})().catch(error => {console.error(error); process.exit(1);});

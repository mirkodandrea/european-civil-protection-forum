// Invisible standalone settings fixture: no server, no live AI, no credentials.
const {chromium} = require(process.env.TTX_PLAYWRIGHT || '../record/node_modules/playwright');
const assert = require('assert/strict');
const fs = require('fs');
const path = require('path');
const storageKey = 'ttx.standalone.openrouter-key';
const token = 'fixture-key-not-a-secret';
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
  const ready = () => page.locator('[data-panel="master"] #start').waitFor({timeout: 60000});
  const settings = page.locator('.standalone-settings');
  try {
    await page.context().setOffline(true);
    await page.goto('file://' + path.resolve(process.env.TTX_STANDALONE_FILE || 'web/standalone/ttx.html'));
    await ready();
    assert.equal(await page.getByLabel('Simulazione', {exact: true}).isChecked(), false);
    assert(await page.locator('.standalone-simulation').isHidden());
    await page.waitForTimeout(500);
    assert.equal(await page.evaluate(() => window.webglStarts), 0, 'disabled view never starts WebGL');
    await page.screenshot({path: path.join(out, 'default-disabled.png')});
    await settings.locator('summary').click();
    await page.locator('#openrouter-key').fill('  ' + token + '  ');
    await page.locator('#configure-ai').click();
    assert.equal(await page.evaluate(key => localStorage.getItem(key), storageKey), token);
    assert.equal(await page.locator('#openrouter-key').inputValue(), '');
    await page.reload();
    await ready();
    assert.match(await settings.innerText(), /AI configurata · chiave salvata nel browser/);
    // Prove the restored token configures real browser AI, rather than just the notice.
    await page.context().setOffline(false);
    let requests = 0;
    await page.route('https://openrouter.ai/api/v1/chat/completions', async route => {
      assert.equal(route.request().headers().authorization, 'Bearer ' + token);
      requests++;
      await route.fulfill({status: 200, contentType: 'application/json', body: JSON.stringify({choices: [{message: {content: JSON.stringify({message:'Issue a warning for F4.', detected_language:'en', translation_en:'Issue a warning for F4.', actions:[{type:'IssueWarning', zone:'F4'}], messages:[], clarification:null, summary:'Warning for F4'})}}]})});
    });
    await page.locator('[data-panel="master"] #start').click();
    await page.locator('.role-tabs [data-role="mayor"]').click();
    const mayor = page.locator('[data-panel="mayor"]');
    await mayor.locator('[data-view="map"]').click();
    await mayor.locator('#new-player-action').click();
    await mayor.getByRole('tab', {name: 'Assistente AI', exact: true}).click();
    await mayor.locator('#work-free-text').fill('Issue a warning for F4.');
    await mayor.getByRole('button', {name: 'Prepara bozza AI →'}).click();
    await mayor.locator('.proposal-preview').filter({hasText: 'Warning for F4'}).waitFor();
    assert(requests > 0);
    await page.getByRole('dialog').locator('header button').click();
    await page.getByRole('dialog').waitFor({state: 'hidden'});
    await settings.locator('summary').click();
    await page.locator('#clear-ai').click();
    assert.equal(await page.evaluate(key => localStorage.getItem(key), storageKey), null);
    await page.reload();
    await ready();
    assert.match(await settings.innerText(), /AI disattivata/);
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
    await page.reload();
    await ready();
    await settings.locator('summary').click();
    await page.locator('#openrouter-key').fill(token);
    await page.locator('#configure-ai').click();
    assert.match(await settings.innerText(), /Archiviazione browser non disponibile/);
    await page.locator('#clear-ai').click();
    assert.match(await settings.innerText(), /impossibile rimuovere/);
    assert.deepEqual(errors, []);
    fs.writeFileSync(path.join(out, 'manifest.json'), JSON.stringify({fixture: 'standalone settings; seed 1; 32 realizations; mocked OpenRouter', checks: ['disabled startup without WebGL', 'save and restore token', 'restored token used by AI', 'remove and reload', 'optional low-quality renderer', 'toggle renderer', 'blocked storage'], canvas, errors}, null, 2));
    console.log('PASS standalone settings:', out);
  } finally {
    await browser.close();
  }
})().catch(error => {console.error(error); process.exit(1);});

// Two real embedded maps, their editor references, activation and saved-session replay.
const {chromium}=require('playwright');
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');

/** The launcher shown before the exercise: resume the saved session, else start the selected scenario. */
async function launch(page) {
  await page.locator('.launcher, [data-panel="master"] #turn-controls').first().waitFor({ state: 'attached', timeout: 60000 });
  const button = page.locator('.launcher #resume-session, .launcher #start-session').first();
  if (await button.count()) await button.click();
}
(async()=>{
 const out=process.env.TTX_QA_OUT || 'runs/territories-qa';fs.mkdirSync(out,{recursive:true});
 const browser=await chromium.launch({headless:true,channel:'chrome'});
 const page=await browser.newPage({viewport:{width:1680,height:1100}}),errors=[];
 page.on('pageerror',e=>{if(!e.message.startsWith('Using exceptions for control flow'))errors.push(e.message)});
 const ready=()=>page.locator('[data-panel="master"] #start').waitFor({timeout:60000});
 const master=page.locator('[data-panel="master"]');
 const scenarios=async()=>{await master.locator('[data-view="scenario"]').click();await page.locator('#scenario-territory').waitFor()};
 const activate=async(id)=>{const row=page.locator(`[data-scenario="${id}"]`);await row.getByRole('button',{name:'Avvia',exact:true}).click();await row.getByRole('button',{name:'Conferma: Avvia',exact:true}).click();await ready()};
 try{
  await page.context().setOffline(true);
  await page.goto('file://'+path.resolve(process.env.TTX_STANDALONE_FILE || '/tmp/ttx-territory-standalone/ttx.html')); await launch(page);await ready();
  await scenarios();assert.equal(await page.locator('#scenario-territory').inputValue(),'');assert.equal(await page.locator('[data-scenario="selvariva_crisi_integrata"]').count(),1);
  await page.locator('#scenario-territory').selectOption('selvariva_basin');
  assert.equal(await page.locator('.scenario-list tbody tr').count(),2);
  await page.screenshot({path:path.join(out,'catalog.png')});
  await page.locator('[data-scenario="selvariva_crisi_integrata"] .edit-scenario').click();
  await page.locator('#scenario-form').waitFor();
  await page.locator('[data-tab="team"]').click();
  await page.screenshot({path:path.join(out,'territory-editor.png')});
  assert.match(await page.locator('#scenario-form').innerText(),/Selvariva/);
  await page.reload(); await launch(page);await ready();await scenarios();await activate('selvariva_crisi_integrata');
  assert.equal(await page.locator('[data-panel="sala_operativa"]').count(),1);
  await scenarios();await page.locator('[data-scenario="selvariva_crisi_integrata"] .edit-scenario').click();
  await page.locator('[data-tab="general"]').click();
  const form=page.locator('#scenario-form');
  assert.equal(await form.locator('input[disabled]').first().inputValue(),'selvariva_basin');
  await page.reload(); await launch(page);await ready();
  await require('./standalone-languages.cjs').confirmLanguages(page);await master.locator('#start').click();await master.locator('#turn').filter({hasText:/orders|ordini/}).waitFor();
  // Autosave must bind the journal to this map, even across a full reload.
  await page.waitForFunction(()=>JSON.parse(localStorage.getItem('ttx.standalone.session')||'null')?.pack.base==='selvariva_basin');
  let session=await page.evaluate(()=>JSON.parse(localStorage.getItem('ttx.standalone.session')));
  assert.match(session.territory_fingerprint,/^selvariva_basin:/);
  await page.screenshot({path:path.join(out,'selvariva-session.png')});
  await page.reload(); await launch(page);await master.locator('#turn').filter({hasText:/orders|ordini/}).waitFor({timeout:60000});
  await scenarios();await page.locator('#scenario-territory').selectOption('valverde_factory');await activate('valverde_factory');
  assert.equal(await page.locator('[data-panel="sala_operativa"]').count(),0);
  await scenarios();assert.equal(await page.locator('#scenario-territory').inputValue(),'');await page.screenshot({path:path.join(out,'valverde-return.png')});
  await page.setViewportSize({width:390,height:844});await page.screenshot({path:path.join(out,'mobile.png')});assert(!await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),'territory list fits mobile');
  assert.deepEqual(errors,[]);fs.writeFileSync(path.join(out,'manifest.json'),JSON.stringify({checks:['two embedded territories','territory filter','editor base reference','activate Selvariva','three roles','session fingerprint','reload replay','return to Valverde'],errors},null,2));
  console.log('PASS territories',out);
 }finally{await browser.close()}
})().catch(e=>{console.error(e);process.exit(1)});

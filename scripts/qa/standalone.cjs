// Invisible fixture. One file:// app, real WASM model, no server or external AI.
const {chromium}=require(process.env.TTX_PLAYWRIGHT||'../record/node_modules/playwright');
const fs=require('fs'),path=require('path'),assert=require('assert/strict'),{execFileSync}=require('child_process');
const out=process.env.TTX_QA_OUT||path.join('runs/standalone-review',new Date().toISOString().replace(/[:.]/g,'-'));
(async()=>{
 fs.mkdirSync(out,{recursive:true});
 const reference=JSON.parse(execFileSync('cargo',['run','--release','--offline','-p','exercise','--example','standalone_reference'],{encoding:'utf8',maxBuffer:8*1024*1024}));
 const browser=await chromium.launch({headless:true,channel:process.env.TTX_BROWSER_CHANNEL||'chrome',args:process.platform==='linux'?['--use-angle=swiftshader','--enable-unsafe-swiftshader']:[]});
 const page=await browser.newPage({viewport:{width:1680,height:1100},deviceScaleFactor:process.platform==='linux'?0.5:1}),errors=[],requests=[],ai=[],browserDiagnostics=[];
 // SwiftShader needs longer for the first full scene; CSS layout remains 1680×1100.
 if(process.platform==='linux')page.setDefaultTimeout(120000);
 page.on('pageerror',e=>{
  // winit transfers control to the browser event loop with this sentinel.
  if(e.message!=="Using exceptions for control flow, don't mind me. This isn't actually an error!") errors.push(e.message);
 });page.on('request',r=>requests.push(r.url()));
 page.on('console',m=>{if(['warning','error'].includes(m.type())){browserDiagnostics.push({type:m.type(),text:m.text()});if(browserDiagnostics.length>200)browserDiagnostics.shift();}});
 page.on('crash',()=>errors.push('Browser page crashed'));
 const master=page.locator('[data-panel="master"]'),mayor=page.locator('[data-panel="mayor"]'),advanced=page.locator('[data-panel="advanced"]');
 const select=async id=>page.locator(`.role-tabs [data-role="${id}"]`).click();
 const download=async name=>{await select('advanced');const pending=page.waitForEvent('download');await advanced.locator('#debriefdownload').click();const d=await pending;await d.saveAs(path.join(out,name));return JSON.parse(fs.readFileSync(path.join(out,name),'utf8'));};
 try {
  await page.context().setOffline(true);
  await page.goto('file://'+path.resolve(process.env.TTX_STANDALONE_FILE||'web/standalone/ttx.html'));
  await master.locator('#start').waitFor({timeout:30000});
  await page.screenshot({path:path.join(out,'desktop.png')});
  await master.locator('#start').click();await master.locator('#turn').filter({hasText:'orders'}).waitFor();
  await select('mayor');await mayor.locator('#language').selectOption('en');await mayor.locator('#join').waitFor({state:'hidden'});
  await select('master');await master.locator('#message-kind').selectOption('inject');await master.locator('#inject-title').fill('Standalone fixture');await master.locator('#text').fill('Issue the warning for F4 and coordinate with the fire commander.');await master.locator('#send').click();await master.locator('#send-status').filter({hasText:'Inviato'}).waitFor();
  console.log('inject delivered');await select('mayor');await mayor.locator('#messages').filter({hasText:'Issue the warning'}).waitFor();
  await mayor.locator('#text').fill('Issue a warning for F4.');await mayor.locator('#send').click();await mayor.locator('#private-preparation').filter({hasText:'Interpretazione non riuscita'}).waitFor();
  await select('master');assert(!await master.locator('#messages').innerText().then(t=>t.includes('Issue a warning for F4.')),'private draft stays private');
  await select('mayor');await mayor.locator('#confirm-prepared').click();await select('master');
  console.log('manual submitted');const proposal=master.locator('[data-proposal]').filter({hasText:'Issue a warning for F4.'});await proposal.waitFor();await proposal.locator('.manual-commands summary').click();await proposal.locator('[data-manual-commands]').fill('[{"type":"IssueWarning","zone":"F4"}]');await proposal.locator('[data-approve]').click();await proposal.filter({hasText:'Approvata'}).waitFor();
  console.log('approved');await master.locator('#minutes').fill('1');await master.locator('#resolve').click();await master.locator('#turn').filter({hasText:'Turno 2 · orders'}).waitFor({timeout:30000});
  await page.screenshot({path:path.join(out,'approved-turn.png')});
  console.log('turn resolved');const resolved=await download('resolved.json');assert.equal(resolved.turns,2);assert.deepEqual(resolved.physical_state,reference.resolved,'paired native and WASM commands produce the same physical state');
  // Real forecast capture/compute/review/publication in WASM.
  const forecast=advanced.locator('#forecast-review');await forecast.getByLabel('Gruppi meteo').fill('1');await forecast.getByLabel('Orizzonti (minuti crescenti)').fill('1');await forecast.locator('[data-compute]').click();await forecast.locator('[data-review]').waitFor({timeout:60000});await forecast.locator('[data-role="mayor"]').check();await forecast.getByLabel('Fonte revisionata e motivazione').fill('Reviewed standalone fixture');await forecast.locator('[data-reviewed]').check();await forecast.locator('[data-publish]').click();await page.waitForFunction(()=>!document.querySelector('[data-panel="advanced"] [data-reviewed]').checked);
  await forecast.locator('h2').scrollIntoViewIfNeeded();await page.screenshot({path:path.join(out,'forecast.png')});
  const rollback=advanced.locator('.card').filter({has:page.locator('h2',{hasText:'Rollback all’inizio del turno'})});
  assert.equal(await rollback.count(),1);await rollback.locator('input').fill('1');await rollback.getByRole('button').click();await select('master');await master.locator('#turn').filter({hasText:'Turno 1 · orders'}).waitFor();
  const rolled=await download('rollback.json');assert.deepEqual(rolled.physical_state,reference.rollback,'rollback restores the same native and WASM state');
  // All role panels coexist in this same document, then map can be hidden.
  await page.getByLabel('Tutti i player',{exact:true}).check();await page.screenshot({path:path.join(out,'all-roles.png')});await page.getByLabel('Tutti i player',{exact:true}).uncheck();
  await page.getByLabel('Simulazione',{exact:true}).uncheck();assert.equal(await page.locator('#standalone-3d').count(),1);assert(await page.locator('.standalone-simulation').isHidden());await page.getByLabel('Simulazione',{exact:true}).check();
  assert(!requests.some(u=>/^https?:|^wss?:/.test(u)),'offline exercise has no network dependencies');
  // Intercept the direct OpenRouter request; no real model or credit consumed.
  await page.context().setOffline(false);
  await page.route('https://openrouter.ai/api/v1/chat/completions',async route=>{
   const body=route.request().postDataJSON(),system=body.messages[0].content;ai.push({model:body.model,authorization:route.request().headers().authorization,task:system.includes('BEFORE sending')?'private':'translate'});
   const content=system.includes('BEFORE sending')?JSON.stringify({message:'Issue a warning for F4.',detected_language:'en',translation_en:'Issue a warning for F4.',actions:[{type:'IssueWarning',zone:'F4'}],messages:[],clarification:null,summary:'Warning for F4'}):body.messages.at(-1).content;
   await route.fulfill({status:200,contentType:'application/json',headers:{'Access-Control-Allow-Origin':'*'},body:JSON.stringify({choices:[{message:{content}}]})});
  });
  await page.locator('.standalone-settings summary').click();await page.locator('#openrouter-key').fill('fixture-key-not-a-secret');await page.locator('#configure-ai').click();assert.equal(await page.locator('#openrouter-key').inputValue(),'');
  await select('mayor');await mayor.locator('#text').fill('Issue a warning for F4.');await mayor.locator('#send').click();await mayor.locator('#private-preparation').filter({hasText:'Warning for F4'}).waitFor({timeout:15000});assert(ai.some(a=>a.task==='private'&&a.authorization==='Bearer fixture-key-not-a-secret'),'AI runs directly from browser with frontend key');
  await page.locator('#clear-ai').click();const callsBeforeClear=ai.length;await mayor.locator('#text').fill('Issue a warning for F5.');await mayor.locator('#send').click();await mayor.locator('#private-preparation').filter({hasText:'Interpretazione non riuscita'}).waitFor();assert.equal(ai.length,callsBeforeClear,'removing key disables later requests');const clean=await download('no-credentials.json');assert(!JSON.stringify(clean).includes('fixture-key-not-a-secret'),'credentials excluded from debrief');
  await page.locator('.standalone-settings summary').click();await select('mayor');await page.setViewportSize({width:820,height:1180});await page.waitForTimeout(300);assert(!await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),'tablet has no horizontal overflow');await page.screenshot({path:path.join(out,'tablet.png')});
  assert.equal(errors.length,0,errors.join('\n'));
  fs.writeFileSync(path.join(out,'manifest.json'),JSON.stringify({fixture:'single file, offline model and controllers; mocked direct OpenRouter fetch',scenario:'valverde_factory.json',seed:1,realizations:32,commands:[{type:'IssueWarning',zone:'F4'}],minutes:1,rollback:1,pairedNativeStateIdentical:true,deviceScaleFactor:process.platform==='linux'?0.5:1,requests,ai:ai.map(({authorization,...r})=>r),errors},null,2));
  console.log(out);console.log('PASS standalone: offline start, inject, private manual fallback, EXCON approval, physics turn, forecast, rollback, views, direct browser AI and secret-free debrief');
 } catch(error) {
  console.error('Original failure:',error);console.log({errors,browserDiagnostics});
  fs.writeFileSync(path.join(out,'failure.json'),JSON.stringify({error:String(error),stack:error.stack,errors,requests,browserDiagnostics},null,2));
  try {await page.screenshot({path:path.join(out,'failure.png'),timeout:10000});}catch(capture){console.warn('Failure screenshot unavailable:',String(capture));}
  throw error;
 } finally {await browser.close();}
})().catch(e=>{console.error(e);process.exit(1)});

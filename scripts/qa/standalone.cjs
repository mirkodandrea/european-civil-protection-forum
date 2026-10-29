// Invisible fixture. One standalone app (file:// build, or TTX_QA_URL for a Trunk
// server), real WASM model, no server or external AI.
const {chromium}=require(process.env.TTX_PLAYWRIGHT||'playwright');
const fs=require('fs'),path=require('path'),assert=require('assert/strict'),{execFileSync}=require('child_process');

/** The start-of-session language gate; a no-op when this session already answered it. */
async function pickLanguage(scope, lang) {
  const choice = scope.locator(`#language-gate [data-lang="${lang}"]`);
  if (await choice.count()) await choice.click();
}
const out=process.env.TTX_QA_OUT||path.join('runs/standalone-review',new Date().toISOString().replace(/[:.]/g,'-'));
const url=process.env.TTX_QA_URL||('file://'+path.resolve(process.env.TTX_STANDALONE_FILE||'web/standalone/ttx.html'));
const origin=url.startsWith('http')?new URL(url).origin:null;
(async()=>{
 fs.mkdirSync(out,{recursive:true});
 const reference=JSON.parse(execFileSync('cargo',['run','--release','--offline','-p','exercise','--example','standalone_reference'],{encoding:'utf8',maxBuffer:8*1024*1024}));
 const browser=await chromium.launch({headless:true,...(process.env.TTX_BROWSER_EXECUTABLE?{executablePath:process.env.TTX_BROWSER_EXECUTABLE}:{channel:process.env.TTX_BROWSER_CHANNEL||'chrome'}),args:process.platform==='linux'?['--use-angle=swiftshader','--enable-unsafe-swiftshader']:[]});
 const page=await browser.newPage({viewport:{width:1680,height:1100},deviceScaleFactor:process.platform==='linux'?0.5:1}),errors=[],requests=[],ai=[],browserDiagnostics=[];
 // SwiftShader needs longer for the first full scene; CSS layout remains 1680×1100.
 if(process.platform==='linux')page.setDefaultTimeout(120000);
 page.on('pageerror',e=>{
  // winit transfers control to the browser event loop with this sentinel.
  if(e.message!=="Using exceptions for control flow, don't mind me. This isn't actually an error!") errors.push(e.message);
 });page.on('request',r=>requests.push(r.url()));
 page.on('console',m=>{if(['warning','error'].includes(m.type())){browserDiagnostics.push({type:m.type(),text:m.text()});if(browserDiagnostics.length>200)browserDiagnostics.shift();}});
 page.on('crash',()=>errors.push('Browser page crashed'));
 const master=page.locator('[data-panel="master"]'),mayor=page.locator('[data-panel="mayor"]');
 const select=async id=>{
  if(['master','timeline','map','risposte'].includes(id)){
   await page.locator('.role-tabs [data-role="master"]').click();
   await master.locator(`[data-view="${id==='master'?'timeline':id}"]`).click();
  }else await page.locator(`.role-tabs [data-role="${id}"]`).click();
 };
 // Controllo (the master "map" view) holds the EXCON tools.
 const tool=async id=>{await select('map');await master.locator(`[data-tool="${id}"]`).click();};
 // A tool opens in a modal: close it (Escape, else its close button).
 const closeTool=async()=>{const b=page.locator('.modal-backdrop');if(!await b.count())return;await page.keyboard.press('Escape');if(await b.count()){const c=b.getByRole('button',{name:/^(Chiudi|Close|×|Annulla)$/}).first();if(await c.count())await c.click();}await b.first().waitFor({state:'detached',timeout:5000}).catch(()=>{});};
 const download=async name=>{await tool('end');const pending=page.waitForEvent('download');await master.locator('#debriefdownload').click();const d=await pending;await d.saveAs(path.join(out,name));await closeTool();return JSON.parse(fs.readFileSync(path.join(out,name),'utf8'));};
 const customInject=async(title,text,recipient)=>{
  await select('timeline');await master.locator('[data-inject-all]').click();const modal=page.getByRole('dialog');
  await modal.locator('#inject-title').fill(title);await modal.locator('#text').fill(text);
  if(recipient){await modal.locator('#inject-scope').selectOption('custom');await modal.getByLabel(recipient).check();}
  await modal.locator('#send').click();await modal.waitFor({state:'hidden'});
 };
 // Answer the inject titled `title` with one decision action (basis: operational judgement).
 const respond=async(role,title,fill)=>{
  const p=page.locator(`[data-panel="${role}"]`);await select(role);await pickLanguage(p,'en');await p.locator('[data-view="map"]').click();
  const back=p.locator('[data-inject-back]');if(await back.count())await back.click();
  // Injects of turn 1 (the turn selector shows one turn at a time).
  await p.locator('#player-timeline header select').selectOption('1');
  await p.locator('[data-inject-row]').filter({hasText:title}).click();await p.locator('[data-reply-direct]').click();
  const work=p.locator('.work-modal');await work.locator('#kind-decision').check();await fill(work);await work.locator('[data-add-action]').click();
  await work.locator('#basis-operational_judgment').check();await work.locator('[data-respond]').click();await work.waitFor({state:'hidden'});
 };
 try {
  await page.goto(url);
  if(origin){await page.evaluate(()=>{localStorage.clear();sessionStorage.clear();});await page.goto(url);}
  await page.locator('.launcher').waitFor({timeout:90000});
  await page.context().setOffline(true);
  await page.locator('.launcher #start-session').click();
  await master.locator('#start').waitFor({timeout:60000});
  assert.equal(await page.getByLabel('Simulazione',{exact:true}).isChecked(),false,'simulation view starts disabled');
  assert(await page.locator('.standalone-simulation').isHidden());
  await page.screenshot({path:path.join(out,'desktop.png')});
  await require('./standalone-languages.cjs').confirmLanguages(page,{mayor:'en',regional_cp:'en'});
  await master.locator('#start').click();await master.locator('#turn').filter({hasText:/orders|ordini/}).waitFor();
  await select('mayor');await pickLanguage(mayor,'en');
  assert.equal(await mayor.locator('[data-view="chat"]').count(),0,'players have no exercise-control chat');
  await mayor.locator('[data-view="map"]').click();
  assert.equal(await mayor.locator('#new-player-action').count(),0,'responses start from an inject (Rispondi)');
  await page.screenshot({path:path.join(out,'player-work.png')});

  // Custom injects: one to the mayor only, one to everybody.
  await customInject('Mayor fixture','Issue the warning for F4 and coordinate with the fire commander.',/Sindaco|Mayor/);
  await customInject('Broadcast fixture','All roles: coordinate on the F4 warning.');
  await page.screenshot({path:path.join(out,'inject-modal-all.png')});
  console.log('targeted and broadcast injects delivered');
  const rows=async role=>{const p=page.locator(`[data-panel="${role}"]`);await select(role);await pickLanguage(p,'en');await p.locator('[data-view="map"]').click();const back=p.locator('[data-inject-back]');if(await back.count())await back.click();return p.locator('.inject-list').innerText();};
  let list=await rows('mayor');assert(list.includes('Mayor fixture')&&list.includes('Broadcast fixture'));
  list=await rows('fire_commander');assert(list.includes('Broadcast fixture')&&!list.includes('Mayor fixture'),'targeted inject stays private to its recipient');

  // The mayor answers the targeted inject with the warning; EXCON approves it in Risposte.
  await respond('mayor','Mayor fixture',async w=>{await w.locator('#work-action').selectOption('IssueWarning');await w.locator('#work-location').fill('F4');});
  console.log('player proposal submitted');
  await select('risposte');await master.locator('[data-response]').filter({hasText:'Mayor fixture'}).filter({hasText:/Sindaco|Mayor/}).locator('.response-row').click();
  const proposal=master.locator('.response-dialog [data-proposal]').first();await proposal.locator('[data-approve]').click();
  await master.locator('.response-dialog [data-response-state="rs_approved"]').waitFor();
  await master.locator('.response-dialog').getByRole('button',{name:/^(Chiudi|Close)$/}).first().click();
  console.log('approved');await page.screenshot({path:path.join(out,'timeline-populated.png')});

  // Close turn 1 after one simulated minute (the native reference resolves 1 minute).
  await select('timeline');await master.locator('#turn-length').fill('1');await master.locator('#turn-length-set').click();
  await master.locator('#advance-next').click();
  // Planned injects crossed by the advance are not sent (their pre-actions would ignite fires).
  // "Advance anyway" leaves them unsent; pre-actions are switched off for the same reason.
  const confirm=page.locator('[data-due-confirm]');
  if(await confirm.waitFor({timeout:5000}).then(()=>true,()=>false)){
   for(let i=0;i<20;i++){const on=page.locator('[data-pre-toggle]:checked');if(!await on.count())break;await on.first().uncheck();await page.waitForTimeout(200);}
   await confirm.click();
  }
  await master.locator('#turn').filter({hasText:/Turno 2|Turn 2/}).waitFor({timeout:60000});
  await page.screenshot({path:path.join(out,'approved-turn.png')});
  console.log('turn resolved');const resolved=await download('resolved.json');assert.equal(resolved.turns,2);assert.deepEqual(resolved.physical_state,reference.resolved,'paired native and WASM commands produce the same physical state');

  // A second role answers the broadcast inject with a request for reinforcements.
  await respond('regional_cp','Broadcast fixture',async w=>{await w.locator('#work-action').selectOption('RequestReinforcement');await w.locator('#work-resource').selectOption('ground_crews');await w.locator('#work-qty').fill('1');});
  await select('risposte');await master.locator('.pending-count').waitFor();

  // The EXCON clock tool opens and closes without changing anything.
  await tool('clock');await page.locator('[data-cancel]').first().click();await closeTool();

  // Real forecast capture/compute/review/publication in WASM.
  await tool('forecast');const forecast=page.locator('#forecast-review');const groups=forecast.getByLabel(/Gruppi meteo|Weather groups/);if(await groups.count())await groups.fill('1');await forecast.getByLabel(/Orizzonti|Horizons/).fill('1');await forecast.locator('[data-compute]').click();await forecast.locator('[data-review]').waitFor({timeout:60000});await forecast.locator('[data-role="mayor"]').check();await forecast.getByLabel(/Fonte revisionata|Reviewed source/).fill('Reviewed standalone fixture');await forecast.locator('[data-reviewed]').check();await forecast.locator('[data-publish]').click();await page.waitForFunction(()=>!document.querySelector('#forecast-review [data-reviewed]')?.checked);
  await page.screenshot({path:path.join(out,'forecast.png')});await closeTool();

  // Rollback to the start of turn 1.
  await tool('rollback');const modal=page.locator('.ctl-modal, [role="dialog"]').last();await modal.locator('input').first().fill('1');await modal.locator('[data-apply]').click();
  await master.locator('#turn').filter({hasText:/Turno 1|Turn 1/}).waitFor();
  const rolled=await download('rollback.json');assert.deepEqual(rolled.physical_state,reference.rollback,'rollback restores the same native and WASM state');

  // All role panels coexist in this same document, then map can be hidden.
  await page.getByLabel('Tutti i player',{exact:true}).check();await page.screenshot({path:path.join(out,'all-roles.png')});await page.getByLabel('Tutti i player',{exact:true}).uncheck();
  await page.getByLabel('Simulazione',{exact:true}).uncheck();assert.equal(await page.locator('#standalone-3d').count(),1);assert(await page.locator('.standalone-simulation').isHidden());
  assert(!requests.some(u=>/^https?:|^wss?:/.test(u)&&!(origin&&u.startsWith(origin))),'offline exercise has no network dependencies');

  // Intercept the direct OpenRouter request; no real model or credit consumed.
  await page.context().setOffline(false);
  await page.route('https://openrouter.ai/api/v1/chat/completions',async route=>{
   const body=route.request().postDataJSON(),system=body.messages[0].content;ai.push({model:body.model,authorization:route.request().headers().authorization,task:system.includes('ONLY what this role knows')?'private':'translate'});
   const content=system.includes('ONLY what this role knows')?JSON.stringify({actions:[{type:'IssueWarning',zone:'F4'}],reasons:['warn F4'],summary:'Warning for F4'}):body.messages.at(-1).content;
   await route.fulfill({status:200,contentType:'application/json',headers:{'Access-Control-Allow-Origin':'*'},body:JSON.stringify({choices:[{message:{content}}]})});
  });
  const settings=async()=>{await page.locator('.role-tabs [data-role="master"]').click();await master.locator('[data-view="scenario"]').click();};
  await settings();await page.locator('#openrouter-key').fill('fixture-key-not-a-secret');await page.locator('#configure-ai').click();
  await select('mayor');await mayor.locator('[data-view="map"]').click();const back=mayor.locator('[data-inject-back]');if(await back.count())await back.click();
  await mayor.locator('[data-inject-row]').filter({hasText:'Broadcast fixture'}).click();await mayor.locator('[data-reply-direct]').click();
  const mayorWork=mayor.locator('#player-work');await mayorWork.locator('#response-narrative').fill('We issue a warning for F4 now.');await mayorWork.locator('[data-add-suggestion="0"]').waitFor({timeout:15000});assert(ai.some(a=>a.task==='private'&&a.authorization==='Bearer fixture-key-not-a-secret'),'AI runs directly from browser with frontend key');
  await mayor.locator('.work-modal header button').click();
  await settings();await page.locator('#clear-ai').click();const callsBeforeClear=ai.length;
  await select('mayor');await mayor.locator('[data-reply-direct]').click();await mayorWork.locator('#response-narrative').fill('We issue a warning for F5 now.');await mayorWork.locator('[data-work-suggestions="ai_unavailable"]').waitFor();assert.equal(ai.length,callsBeforeClear,'removing key disables later requests');
  await mayor.locator('.work-modal header button').click();
  const clean=await download('no-credentials.json');assert(!JSON.stringify(clean).includes('fixture-key-not-a-secret'),'credentials excluded from debrief');
  await select('mayor');await page.setViewportSize({width:820,height:1180});await page.waitForTimeout(300);assert(!await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),'tablet has no horizontal overflow');await page.screenshot({path:path.join(out,'tablet.png')});
  assert.equal(errors.length,0,errors.join('\n'));
  fs.writeFileSync(path.join(out,'manifest.json'),JSON.stringify({fixture:'single standalone app, offline model and controllers; mocked direct OpenRouter fetch',url,scenario:'valverde_factory.json',seed:1,realizations:32,commands:[{type:'IssueWarning',zone:'F4'}],minutes:1,rollback:1,pairedNativeStateIdentical:true,deviceScaleFactor:process.platform==='linux'?0.5:1,requests:requests.filter(u=>!(origin&&u.startsWith(origin))),ai:ai.map(({authorization,...r})=>r),errors},null,2));
  console.log(out);console.log('PASS standalone: offline start, injects, Rispondi with action, EXCON approval in Risposte, physics turn, forecast, rollback, views, direct browser AI and secret-free debrief');
 } catch(error) {
  console.error('Original failure:',error);console.log({errors,browserDiagnostics:browserDiagnostics.slice(-10)});
  fs.writeFileSync(path.join(out,'failure.json'),JSON.stringify({error:String(error),stack:error.stack,errors,requests,browserDiagnostics},null,2));
  try {await page.screenshot({path:path.join(out,'failure.png'),timeout:10000});}catch(capture){console.warn('Failure screenshot unavailable:',String(capture));}
  throw error;
 } finally {await browser.close();}
})().catch(e=>{console.error(e);process.exit(1)});

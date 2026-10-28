// Local fake model: exercises the real HTTP pipeline without external AI calls.
const {chromium}=require(process.env.TTX_PLAYWRIGHT||'../record/node_modules/playwright');
const http=require('http'),fs=require('fs'),path=require('path'),assert=require('assert/strict'),{spawn}=require('child_process');
const cwd=path.resolve(__dirname,'../..'),out=process.env.TTX_QA_OUT||path.join(cwd,'runs/private-preparation-review');
(async()=>{
 fs.mkdirSync(out,{recursive:true});
 const requests=[],errors=[];
 const gateway=http.createServer(async(req,res)=>{
  let raw='';for await(const chunk of req)raw+=chunk;
  const body=JSON.parse(raw),system=body.messages[0].content;
  const privateTask=system.includes('BEFORE sending');
  requests.push({model:body.model,task:privateTask?'prepare_decision':'translate',providers:body.provider});
  let content;
  if(privateTask){
   const user=body.messages.at(-1).content,complete=user.includes('PLAYER: F4');
   content=JSON.stringify({message:complete?"Emetti un’allerta per F4.":'Avvisa la zona',detected_language:'it',translation_en:complete?'Issue a warning for F4.':'Warn the area',actions:complete?[{type:'IssueWarning',zone:'F4'}]:[],messages:[],clarification:complete?null:'Quale zona deve ricevere l’allerta?',summary:complete?'Allerta per F4':'Manca la zona da allertare'});
  }else content=body.messages.at(-1).content;
  res.writeHead(200,{'Content-Type':'application/json'});res.end(JSON.stringify({choices:[{message:{content}}]}));
 });
 await new Promise(resolve=>gateway.listen(0,'127.0.0.1',resolve));
 const port=process.env.TTX_QA_PORT||'8096',base=`http://127.0.0.1:${port}`;
 const child=spawn(process.env.TTX_BINARY||path.join(cwd,'target/release/ttx'),['--headless','--port',port,'--realizations','1'],{cwd,env:{...process.env,OPENROUTER_API_KEY:'local-fixture-only',OPENROUTER_BASE_URL:`http://127.0.0.1:${gateway.address().port}/v1`,OPENROUTER_MODEL:'deepseek/deepseek-v4.1-flash-1017',OPENROUTER_TRANSLATION_MODEL:'google/gemini-3.1-flash-lite',TTX_LLM_CASSETTE:''}});
 let browser,key,log='';
 const ready=new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('startup timeout '+log)),20000);const read=data=>{log+=data;const match=log.match(/EXCON access:.*token=([a-f0-9]+)/);if(match){key=match[1];clearTimeout(timer);resolve();}};child.stdout.on('data',read);child.stderr.on('data',read);child.on('exit',code=>{clearTimeout(timer);reject(Error(`server exited ${code}: ${log}`));});});
 try{
  await ready;
  const api=async(route,body)=>{const response=await fetch(base+route,{method:body===undefined?'GET':'POST',headers:{Authorization:`Bearer ${key}`,'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)});const result=await response.json();assert.equal(response.status,200,JSON.stringify(result));return result;};
  await api('/api/master/start',{});await api('/api/player/mayor/language',{language:'it'});
  await api('/api/master/inject',{title:'Segnalazione dal territorio',sender:'112 Dispatch',recipients:['mayor'],text:'Smoke reported near Pineta.'});
  const links=await api('/api/master/access'),mayor=links.players.find(role=>role.role==='mayor');
  const before=(await api('/api/state')).world;
  browser=await chromium.launch({headless:true,channel:'chrome'});
  const player=await browser.newPage({viewport:{width:800,height:1280},hasTouch:true,isMobile:true,deviceScaleFactor:1});
  player.on('pageerror',error=>errors.push(error.message));
  await player.goto(base+mayor.url);await player.locator('#messages').filter({hasText:'Smoke reported'}).waitFor();
  await player.locator('#text').fill('Avvisa la zona');await player.locator('#send').click();
  await player.locator('.draft-question').filter({hasText:'Quale zona deve ricevere'}).waitFor();
  assert.equal(await player.locator('#confirm-prepared').count(),0);
  let master=await api('/api/master');assert.equal(master.proposals.length,0);assert(!JSON.stringify(master).includes('Avvisa la zona'));
  await player.screenshot({path:path.join(out,'player-private-question-portrait.png')});
  await player.locator('#text').fill('F4');await player.locator('#send').click();
  await player.locator('#confirm-prepared').waitFor();
  master=await api('/api/master');assert.equal(master.proposals.length,0);assert(!master.messages.some(message=>message.original==='Emetti un’allerta per F4.'));
  const canonical=await player.locator('#text').inputValue();
  await player.locator('#text').fill(canonical+' modifica');assert(await player.locator('#confirm-prepared').isDisabled(),'editing invalidates confirmation');
  await player.locator('#text').fill(canonical);assert(!(await player.locator('#confirm-prepared').isDisabled()));
  await player.setViewportSize({width:1280,height:800});await player.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
  const confirm=await player.locator('#confirm-prepared').boundingBox();assert(confirm.y+confirm.height<=800,'tablet confirmation is visible');
  await player.screenshot({path:path.join(out,'player-private-ready-landscape.png')});
  await player.locator('#confirm-prepared').click();await player.locator('#messages').filter({hasText:'Emetti un’allerta per F4.'}).waitFor();
  master=await api('/api/master');assert.equal(master.proposals.length,1);assert.equal(master.proposals[0].status,'pending');assert.equal(master.proposals[0].commands[0].zone,'F4');
  assert(!JSON.stringify(master).includes('Quale zona deve ricevere'),'private assistant questions stay private after submission');
  assert.deepEqual((await api('/api/state')).world,before);
  assert.equal(requests.filter(request=>request.task==='prepare_decision').length,2,'confirmation reuses the reviewed interpretation');
  assert(requests.filter(request=>request.task==='translate').length>0);
  assert(requests.filter(request=>request.task==='translate').every(request=>request.model==='google/gemini-3.1-flash-lite'));
  assert(requests.filter(request=>request.task==='prepare_decision').every(request=>request.model==='deepseek/deepseek-v4.1-flash-1017'));
  const excon=await browser.newPage({viewport:{width:1440,height:1000}});excon.on('pageerror',error=>errors.push(error.message));
  await excon.goto(base+'/master.html?token='+key);await excon.locator('#messages').filter({hasText:'Issue a warning for F4.'}).waitFor();
  await excon.screenshot({path:path.join(out,'excon-only-confirmed-decision.png')});
  await excon.locator('#messages [data-proposal]').filter({hasText:'Issue a warning for F4.'}).waitFor();
  await excon.screenshot({path:path.join(out,'excon-reviewed-translation.png')});
  assert.equal(errors.length,0,errors.join('\n'));
  fs.writeFileSync(path.join(out,'manifest.json'),JSON.stringify({fixture:'local fake OpenRouter gateway; no external model calls',actions:['inject','private ambiguous draft','specific question to player','answer F4','review canonical message and actions','explicit confirmation','pending EXCON proposal'],requests,errors},null,2));
  console.log('PASS: private question, clarification, exact-message review, explicit submission, unchanged physics; separate translation model; no external AI calls');
 }finally{await browser?.close();child.kill('SIGTERM');gateway.closeAllConnections();await new Promise(resolve=>gateway.close(resolve));}
})().catch(error=>{console.error(error);process.exitCode=1;});

// Run after cargo build --release -p app. Uses the existing recording Playwright install.
const { chromium } = require('../record/node_modules/playwright');
const path=require('path');
const {spawn}=require('child_process');
const cwd=path.resolve(__dirname,'../..');
(async()=>{
 const child=spawn(cwd+'/target/release/ttx',['--headless','--port','8098','--realizations','1','--operational-model'],{cwd,env:{...process.env,OPENROUTER_API_KEY:''}});
 let output='',key;
 const ready=new Promise((resolve,reject)=>{
  const timer=setTimeout(()=>reject(Error('server start timeout')),15000);
  const read=data=>{output+=data;const m=output.match(/EXCON access:.*token=([a-f0-9]+)/);if(m){key=m[1];clearTimeout(timer);resolve();}};
  child.stdout.on('data',read);child.stderr.on('data',read);child.on('exit',code=>reject(Error('server exited '+code)));
 });
 let browser;
 try {
  await ready;
  require('fs').mkdirSync(path.join(cwd,'runs/visual-review'),{recursive:true});
  browser=await chromium.launch({headless:true,channel:"chrome"});
  const page=await browser.newPage({viewport:{width:1440,height:1100},deviceScaleFactor:1});
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.goto('http://localhost:8098/master.html?token='+key);
  await page.locator('[data-compute]').waitFor();
  await page.locator('#clock').filter({hasText:'Day'}).waitFor();
  await page.locator('[data-members]').fill('1');await page.locator('[data-horizons]').fill('1,2');
  await page.locator('[data-compute]').click();
  await page.locator('[data-status]').filter({hasText:'Private draft ready'}).waitFor({timeout:90000});
  await page.locator('#forecast-review').screenshot({path:cwd+'/runs/visual-review/forecast-desktop.png'});
  await page.locator('[data-role]').first().check();await page.locator('[data-source]').fill('Controlled QA bulletin review');await page.locator('[data-reviewed]').check();
  if(await page.locator('[data-publish]').isDisabled())throw Error('publish remained disabled');
  await page.locator('[data-publish]').click();await page.locator('[data-status]').filter({hasText:'published to the selected roles'}).waitFor();
  await page.setViewportSize({width:390,height:844});await page.locator('#forecast-review').screenshot({path:cwd+'/runs/visual-review/forecast-mobile.png'});
  if(errors.length)throw Error(errors.join('\n'));
  console.log('PASS: private draft, preview, reviewed targeted publication, desktop/mobile; no page errors');
 }finally{await browser?.close();child.kill('SIGTERM');}
})().catch(e=>{console.error(e.message);process.exitCode=1;});

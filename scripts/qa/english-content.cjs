// English content check: launcher in English, Selvariva pack, every role in
// English; walks EXCON and player views and reports lines that still look
// Italian. Usage: TTX_QA_URL=http://127.0.0.1:8086 node scripts/qa/english-content.cjs
// Map names (places, buildings, roads) are territory identifiers and stay as authored.
const {chromium}=require(process.env.TTX_PLAYWRIGHT||'playwright');
const fs=require('fs'),path=require('path');
const url=process.env.TTX_QA_URL||'http://127.0.0.1:8086';
const pack=process.env.TTX_QA_PACK||'selvariva_crisi_integrata';
const lang=process.env.TTX_QA_LANG||'en';
const out=process.env.TTX_QA_OUT||path.join('runs/english-content',new Date().toISOString().replace(/[:.]/g,'-'));
const ITALIAN=/\b(della|delle|degli|nella|nelle|sulla|alla|non|una|il|gli|che|è|può|sono|zona|squadre|incendio|risorse|ruolo|strada|edificio|turno|nessun[ao]?|già|ancora|questo|questa|chiudi|salva|elimina|aggiungi|annulla|invia|apri|scegli|esercitazione|mezzi|rinforzi|evacuazione|avviso|luogo)\b/i;
// Territory toponyms that legitimately stay Italian.
const NAMES=/Selvariva|Roverechiare|Pianalto|Fonti|Fornaci|Roveri|Prati|Montebruno|Due Valli|Corriere della Conca|Rifugio della scolaresca|Presidio sanitario|Municipio|Scuola di|Polo di accoglienza|Centro (di accoglienza|operativo)|Stazione carburanti|Strada del|Via d|Raccordo|Accesso|Sentiero|Margine|Piazza di|Cascina|Bosco delle|Conca|Area festival|Deposito delle/;
(async()=>{
 fs.mkdirSync(out,{recursive:true});
 const browser=await chromium.launch({headless:true,channel:process.env.TTX_BROWSER_CHANNEL||'chrome'});
 const page=await browser.newPage({viewport:{width:1600,height:1000}});
 const errors=[];page.on('pageerror',e=>{if(!/control flow/.test(e.message))errors.push(e.message);});
 const report=[];
 const grab=async(name,scope)=>{
  await page.waitForTimeout(400);
  await page.screenshot({path:path.join(out,name+'.png')});
  const text=await (scope||page.locator('body')).innerText();
  fs.writeFileSync(path.join(out,name+'.txt'),text);
  const hits=text.split('\n').map(l=>l.trim()).filter(l=>l&&ITALIAN.test(l.replace(NAMES,'')));
  report.push({name,hits:[...new Set(hits)]});
 };
 await page.goto(url);
 await page.evaluate(()=>{localStorage.clear();sessionStorage.clear();});
 await page.goto(url);
 await page.locator('.launcher').waitFor({timeout:120000});
 await page.locator(`#launch-excon-lang [data-lang="${lang}"]`).click();
 await grab('launcher');
 await page.locator(`.launcher input[value="${pack}"]`).check();
 await page.locator('.launcher #start-session').click();
 const master=page.locator('[data-panel="master"]');
 await master.locator('#start').waitFor({timeout:120000});
 await require('./standalone-languages.cjs').confirmLanguages(page,Object.fromEntries((await page.locator('.role-tabs [data-role]').evaluateAll(t=>t.map(x=>x.dataset.role))).map(r=>[r,lang])));
 await grab('excon-before-start',master);
 await master.locator('#start').click();
 await page.waitForTimeout(1500);
 for(const v of ['timeline','situazione','risposte','map','history','scenario']){
  const tab=master.locator(`[data-view="${v}"]`);if(!await tab.count())continue;
  await tab.first().click();await grab('excon-'+v,master);
 }
 // Send the first due inject, then read it as the players do.
 await master.locator('[data-view="timeline"]').first().click();
 const send=master.getByRole('button',{name:/^(Send|Invia)$/}).first();
 if(await send.count()){await send.click();const m=page.getByRole('dialog');if(await m.count()){const b=m.locator('#send');if(await b.count())await b.click();}await page.waitForTimeout(1500);await grab('excon-after-send',master);}
 const roles=await page.locator('.role-tabs [data-role]').evaluateAll(t=>t.map(x=>x.dataset.role).filter(r=>r!=='master'));
 for(const r of roles){
  await page.locator(`.role-tabs [data-role="${r}"]`).click();
  const p=page.locator(`[data-panel="${r}"]`);
  for(const v of ['function','map']){const tab=p.locator(`[data-view="${v}"]`);if(await tab.count()){await tab.first().click();await grab(`player-${r}-${v}`,p);}}
 }
 fs.writeFileSync(path.join(out,'report.json'),JSON.stringify({errors,report},null,1));
 for(const r of report)if(r.hits.length){console.log('== '+r.name);for(const h of r.hits)console.log('   '+h.slice(0,200));}
 console.log('errors:',errors.length,'out:',out);
 await browser.close();
})().catch(e=>{console.error(e);process.exit(1);});

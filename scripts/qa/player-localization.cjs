// Localization audit of every player screen (02/11 playtest: "excon e observer
// solo ita/eng, il resto va tutto localizzato"). All players in Ukrainian
// (Cyrillic) and the LLM mocked with a Latin->Cyrillic transliteration, so
// translated content comes out Cyrillic and any Latin word left on a player
// panel is untranslated UI. Walks the language gate, briefing, empty orders,
// inject list and detail, the reply composer in every state, a sent reply, a
// conversation (starter and guest), an EXCON question and its answer, a time
// advance, notifications, legend and map. Canvas text (map labels) is not
// read: check the screenshots. Fails on Latin words outside the allowlist.
//   ./scripts/dev_frontend.sh --port 8093
//   TTX_QA_URL=http://127.0.0.1:8093/ node scripts/qa/player-localization.cjs [OUT_DIR]
// AUDIT_LANG: another non-Latin player language (el, hy, ka).
const {chromium}=require(process.env.TTX_PLAYWRIGHT||'playwright');
const fs=require('fs'),path=require('path');
const QA=__dirname;
const out=process.argv[2]||path.join(__dirname,'../../runs/player-localization',new Date().toISOString().replace(/[:.]/g,'-'));
const url=process.env.TTX_QA_URL||'http://127.0.0.1:8093/';
const LANG=process.env.AUDIT_LANG||'uk';
fs.mkdirSync(out,{recursive:true});

const MAP={a:'а',b:'б',c:'ц',d:'д',e:'е',f:'ф',g:'г',h:'х',i:'і',j:'й',k:'к',l:'л',m:'м',n:'н',o:'о',p:'п',q:'к',r:'р',s:'с',t:'т',u:'у',v:'в',w:'в',x:'кс',y:'и',z:'з'};
// Keep zone codes (F4, F4:N), road ids (SP12) and numbers as they are.
const translit=s=>s.replace(/\b(?![A-H][1-8]\b)(?!SP\s?\d)[A-Za-zÀ-ÿ’']+/g,w=>[...w.normalize('NFD').replace(/[̀-ͯ]/g,'')].map(c=>{const m=MAP[c.toLowerCase()];if(!m)return c;return c===c.toLowerCase()?m:m.toUpperCase();}).join(''));

(async()=>{
  const browser=await chromium.launch({headless:true,channel:'chrome'});
  const page=await browser.newPage({viewport:{width:1600,height:1000}}),errors=[],tasks=[];
  page.on('pageerror',e=>{if(!e.message.includes('control flow'))errors.push(e.message);});
  await page.route('https://openrouter.ai/api/v1/chat/completions',async route=>{
    const req=route.request().postDataJSON();
    const system=req.messages?.[0]?.content||'',user=req.messages?.at(-1)?.content||'';
    let content;
    if(system.includes('labels of a map')){tasks.push('names');let names=[];try{names=JSON.parse(user);}catch{}content=JSON.stringify({translations:Object.fromEntries(names.map(n=>[n,translit(n)]))});}
    else if(system.startsWith('Translate')&&system.includes('to English')){tasks.push('to_en');content=user;}
    else if(system.startsWith('Translate')){tasks.push('translate');content=translit(user);}
    else if(system.includes('Players never act on the simulator')){tasks.push('review');
      const author=(user.match(/"author"\s*:\s*"([a-z_]+)"/)||user.match(/"role"\s*:\s*"([a-z_]+)"/)||[])[1]||'mayor';
      content=JSON.stringify({actions:[{type:'CutFireLine',role:'fire_commander',params:{},quote:'',notes:['Endpoints not stated.']}],questions:[{to:author,text:'Between which points and with which resources?',about:[0],reason:'gap'}],summary:'A reply without details.'});}
    else if(/JSON/.test(system)){tasks.push('json');content=JSON.stringify({actions:[],messages:[],questions:[],summary:'',clarification:null});}
    else {tasks.push('text');content=translit('Turn summary: the fire advanced, crews deployed, two buildings threatened.');}
    await route.fulfill({status:200,contentType:'application/json',headers:{'Access-Control-Allow-Origin':'*'},body:JSON.stringify({choices:[{message:{content}}]})});
  });
  const panel=r=>page.locator(`[data-panel="${r}"]`),master=panel('master');
  const choose=r=>page.locator(`.role-tabs [data-role="${r}"]`).click();
  const found=new Map();// text -> {states:Set, where:Set}
  let n=0;
  const harvest=async(state,role)=>{
    await page.waitForTimeout(500);
    const items=await panel(role).evaluate(root=>{
      const out=[];
      const where=el=>{const p=[];for(let e=el;e&&e!==root&&p.length<4;e=e.parentElement){p.unshift(e.tagName.toLowerCase()+(e.id?'#'+e.id:'')+(e.classList.length?'.'+[...e.classList].slice(0,2).join('.'):''));}return p.join('>');};
      const w=document.createTreeWalker(root,NodeFilter.SHOW_TEXT);
      while(w.nextNode()){const t=w.currentNode.textContent.trim();const el=w.currentNode.parentElement;if(t&&el&&!['SCRIPT','STYLE'].includes(el.tagName))out.push({t,w:where(el)});}
      for(const el of root.querySelectorAll('[placeholder],[title],[aria-label],[alt]'))for(const a of ['placeholder','title','aria-label','alt']){const v=el.getAttribute(a);if(v&&v.trim())out.push({t:v.trim(),w:'@'+a+' '+where(el)});}
      return out;
    });
    for(const {t,w} of items){
      if(!/[A-Za-z]{2,}/.test(t))continue;
      const k=t.slice(0,220);
      if(!found.has(k))found.set(k,{states:new Set(),where:new Set()});
      found.get(k).states.add(`${role}:${state}`);found.get(k).where.add(w);
    }
    await page.screenshot({path:path.join(out,`${String(++n).padStart(2,'0')}-${role}-${state}.png`)});
  };
  const views=async(state,roles)=>{for(const r of roles){await choose(r);for(const v of ['function','map']){const b=panel(r).locator(`[data-view="${v}"]`);if(await b.count()){await b.click();await harvest(`${state}-${v}`,r);}}}};
  let failed=0;
  const step=async(name,fn)=>{try{await fn();console.log('ok',name);}catch(e){failed++;console.log('FAILED',name,e.message.split('\n')[0]);await page.screenshot({path:path.join(out,`fail-${name}.png`)}).catch(()=>{});}};
  let players=[];
  await step('launch',async()=>{
    await page.goto(url,{waitUntil:'domcontentloaded'});
    await page.evaluate(()=>{localStorage.clear();sessionStorage.clear();});
    await page.goto(url,{waitUntil:'domcontentloaded'});
    await page.locator('.launcher').waitFor({timeout:90000});
    await page.locator('#launch-openrouter-key').fill('qa-fixture-only');
    await page.locator('#launch-save-key').click();
    await page.locator('.launcher #start-session').click();
    await master.locator('#start').waitFor({timeout:60000});
    const sim=page.getByLabel('Simulazione',{exact:true});if(await sim.count()&&await sim.isChecked())await sim.uncheck();
    players=(await page.locator('.role-tabs [data-role]').evaluateAll(t=>t.map(x=>x.dataset.role))).filter(r=>r!=='master'&&!/observ/i.test(r));
    console.log('players',players);
  });
  if(failed){console.log('the app did not start');await browser.close();process.exit(1);}
  await step('language-gate',async()=>{
    await choose(players[0]);await panel(players[0]).locator('#language-gate').waitFor({timeout:5000});await harvest('language-gate',players[0]);
    await require(path.join(QA,'standalone-languages.cjs')).confirmLanguages(page,Object.fromEntries(players.map(p=>[p,LANG])));
  });
  await step('briefing',()=>views('briefing',players));
  await step('start',async()=>{await choose('master');await master.locator('#start').click();await master.locator('#turn').filter({hasText:/orders|ordini/i}).waitFor();});
  await step('orders-empty',()=>views('orders-empty',players));
  await step('inject',async()=>{
    await choose('master');await master.locator('[data-view="timeline"]').click();await master.locator('[data-inject-all]').click();
    const modal=page.getByRole('dialog');
    await modal.locator('#inject-title').fill('Smoke above Le Terrazze');
    await modal.locator('#inject-sender').fill('Emergency call centre (112)');
    await modal.locator('#text').fill('Several residents report a column of smoke above the pine wood.');
    await modal.locator('#send').click();await modal.waitFor({state:'hidden'});
  });
  await step('inject-list',()=>views('inject-list',players));
  for(const r of players){
    await step(`composer-${r}`,async()=>{
      await choose(r);const p=panel(r);await p.locator('[data-view="map"]').click();
      const back=p.locator('[data-inject-back]');if(await back.count())await back.click();
      await harvest('inject-list-only',r);
      await p.locator('[data-inject-row]').first().click();await p.locator('[data-reply-composer]').waitFor();
      await harvest('composer',r);
      const more=p.locator('.reply-guide-more summary');if(await more.count()){await more.click();await harvest('composer-guide',r);await more.click();}
      await p.locator('#basis-plan').click();await harvest('composer-plan',r);
      await p.locator('#basis-law').click();await harvest('composer-law',r);await p.locator('#basis-law').click();
      await p.locator('[data-reply-composer] [data-to="players"]').check();await harvest('composer-players',r);
      const person=p.locator('[data-conv-person]').first();if(await person.count()){await person.check();await harvest('composer-players-picked',r);}
      await p.locator('[data-reply-composer] [data-to="sender"]').check();
    });
  }
  const [A,B]=players;
  await step('reply-sent',async()=>{
    const p=panel(A);await choose(A);
    await p.locator('#response-narrative').fill('We open the gym in F4 as a waiting area.');
    await p.locator('#basis-plan').click();await p.locator('[data-basis-note]').fill('Municipal plan, card 4');
    await p.locator('[data-respond]').click();await page.waitForTimeout(1500);
    await harvest('reply-sent',A);
  });
  await step('conversation',async()=>{
    const p=panel(B);await choose(B);await p.locator('[data-view="map"]').click();
    const back=p.locator('[data-inject-back]');if(await back.count())await back.click();
    await p.locator('[data-inject-row]').first().click();
    await p.locator('[data-reply-composer] [data-to="players"]').check();
    await p.locator(`[data-conv-person="${A}"]`).check();
    await p.locator('#response-narrative').fill('Can you close the SP12 before we evacuate F4?');
    await p.locator('#basis-law').click();await p.locator('[data-basis-note]').fill('Law 1/2018 art. 12');
    await p.locator('[data-rationale]').fill('The road must be free first.');
    await p.locator('[data-send-conversation]').click();await p.locator('[data-conversation]').waitFor();
    await harvest('conversation-started',B);
    const a=panel(A);await choose(A);await a.locator('[data-view="map"]').click();
    const back2=a.locator('[data-inject-back]');if(await back2.count())await back2.click();
    await harvest('list-with-conversation',A);
    const row=a.locator('[data-conv-row]').first();if(await row.count()){await row.click();await harvest('conversation-guest',A);}
    else{await a.locator('[data-inject-row]').first().click();await harvest('inject-with-conversation',A);const r2=a.locator('[data-conv-row]').first();if(await r2.count()){await r2.click();await harvest('conversation-guest',A);}}
  });
  await step('question',async()=>{
    await choose('master');await master.locator('[data-view="risposte"]').click();
    const board=master.locator('#review-board');await board.waitFor();
    const q=board.locator('[data-question-state="draft"]').first();await q.waitFor({timeout:30000});
    await q.locator('[data-review-send]').click();await board.locator('[data-question-state="sent"]').first().waitFor();
    const to=players.find(()=>true);
    for(const r of players){await choose(r);const p=panel(r);await p.locator('[data-view="map"]').click();const back=p.locator('[data-inject-back]');if(await back.count())await back.click();
      if(await p.locator('[data-send-clarification]').count()===0){await p.locator('[data-inject-row]').first().click();}
      if(await p.locator('[data-send-clarification]').count()){await harvest('question',r);
        await p.locator('.clarify-reply textarea').fill('From E3 to F3 with the bulldozer.');await p.locator('[data-send-clarification]').click();await page.waitForTimeout(1500);await harvest('question-answered',r);}}
  });
  await step('advance',async()=>{
    await choose('master');await master.locator('[data-view="timeline"]').click();
    await master.locator('#advance-step').fill('30');await master.locator('#advance-step-go').click();
    const confirm=page.locator('[data-due-confirm]');
    if(await confirm.waitFor({timeout:5000}).then(()=>true,()=>false)){for(let i=0;i<20;i++){const on=page.locator('[data-pre-toggle]:checked');if(!await on.count())break;await on.first().uncheck();await page.waitForTimeout(200);}await confirm.click();}
    await page.waitForTimeout(3000);
    await harvest('resolving-map',A).catch(()=>{});
    await master.locator('#turn').filter({hasText:/orders|ordini|debrief/i}).waitFor({timeout:180000});
  });
  await step('after-advance',()=>views('after-advance',players));
  await step('bell-legend',async()=>{
    for(const r of players){await choose(r);const p=panel(r);await p.locator('[data-view="map"]').click();
      const bell=p.locator('.notification-bell, [data-notification-bell], button[aria-label*="otif"], .bell').first();
      if(await bell.count()){await bell.click();await harvest('notifications',r);await page.keyboard.press('Escape');}
      const legend=p.locator('.map-legend-toggle, [data-legend-toggle], button[title*="egend"]').first();
      if(await legend.count()){await legend.click();await harvest('legend',r);}
      const canvas=p.locator('.tactical-map canvas, .tactical-map svg').first();
      if(await canvas.count()){const b=await canvas.boundingBox();if(b){await page.mouse.click(b.x+b.width*0.55,b.y+b.height*0.62);await harvest('map-click',r);}}
    }
  });
  // Report.
  const rows=[...found.entries()].map(([t,v])=>({t,states:[...v.states],where:[...v.where].slice(0,3)}));
  fs.writeFileSync(path.join(out,'latin.json'),JSON.stringify(rows,null,1));
  // Allowed: the brand, the loanwords the catalogues keep (inject, EXCON),
  // zone and road codes, key names, and what the participants typed here.
  const TYPED=['We open the gym','Municipal plan','Law 1/2018','The road must be free','Can you close the SP12','From E3 to F3'];
  const residue=t=>t.replace(/PPRD East 4 · TTX/g,'').replace(/\b(inject|EXCON|Ctrl\+Enter)\b/gi,'').replace(/\b[A-H][1-8](:[NSEW])?\b|\bSP\s?\d+\b/g,'');
  const bad=rows.filter(r=>!r.states.every(s=>s.endsWith(':language-gate'))&&!TYPED.some(x=>r.t.includes(x))&&/[A-Za-z]{2,}/.test(residue(r.t)));
  for(const r of bad)console.log('UNTRANSLATED',JSON.stringify(r.t.slice(0,120)),'·',r.states[0],'·',r.where[0]);
  if(bad.length||errors.length||failed)process.exitCode=1;
  console.log('tasks',tasks.reduce((a,t)=>(a[t]=(a[t]||0)+1,a),{}));
  console.log('errors',errors);
  console.log(`${rows.length} Latin texts; report ${path.join(out,'latin.json')}`);
  await browser.close();
})();

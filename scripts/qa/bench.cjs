// Workflow test bench ("Banco di prova"): clicks through every bench with the
// real buttons and checks that each step reaches `data-status="done"`.
// Serve a standalone+bench build first (scripts/dev_bench.sh, or `trunk build
// --features standalone,bench` + any static server) and pass its URL:
//   TTX_BENCH_URL=http://127.0.0.1:8080/ node scripts/qa/bench.cjs
// TTX_QA_OUT: screenshot directory (default runs/bench-review/<time>).
const {chromium}=require(process.env.TTX_PLAYWRIGHT||'playwright');
const fs=require('fs'),path=require('path'),assert=require('assert/strict');

const base=(process.env.TTX_BENCH_URL||'http://127.0.0.1:8080/').replace(/\?.*$/,'');
const out=process.env.TTX_QA_OUT||path.join('runs/bench-review',new Date().toISOString().replace(/[:.]/g,'-'));

/** Waits until step `n` of the open bench is done. */
async function done(page,n){
  await page.locator(`[data-bench-step="${n}"][data-status="done"]`).waitFor({timeout:10000});
}
async function allDone(page,id){
  const steps=page.locator('[data-bench-step]');
  const n=await steps.count();
  for(let i=1;i<=n;i++)await done(page,i);
  console.log(`${id}: ${n} steps done`);
}
const col=(page,id)=>page.locator(`[data-bench-col="${id}"]`);
/** Opens the (only) inject of a player column. */
async function openInject(page,role){
  await col(page,role).locator('[data-inject-row]').first().click();
}
/** Opens the response row of `role` in the EXCON Risposte list. */
async function openResponse(page,role){
  await col(page,'excon').locator(`[data-response$=":${role}"] .response-row`).first().click();
}

const benches={
  async inject(page){
    const excon=col(page,'excon'),mayor=col(page,'mayor');
    await excon.locator('[data-bench-send="I1"]').click();
    await excon.locator('[data-send]').click();await done(page,1);
    await openInject(page,'mayor');await done(page,2);
    await mayor.locator('[data-reply-inject]').click();await mayor.locator('[data-reply-direct]').click();
    await mayor.locator('#response-narrative').fill('Apriamo il centro operativo comunale e informiamo la popolazione.');
    await mayor.locator('#kind-decision').check();await mayor.locator('#basis-operational_judgment').check();
    await mayor.locator('[data-respond]').click();await done(page,3);
    await openResponse(page,'mayor');await excon.locator('[data-approve]').click();
  },
  async suggest(page){
    const police=col(page,'police');
    await openInject(page,'police');await police.locator('[data-reply-inject]').click();await police.locator('[data-reply-direct]').click();await done(page,1);
    await police.locator('#response-narrative').fill('Chiudiamo la SP12 ai curiosi e avvisiamo la zona E3.');
    await police.locator('[data-add-suggestion="0"]').click();await done(page,2);
    await police.locator('#basis-operational_judgment').check();
    await police.locator('[data-respond]').click();await done(page,3);
    await col(page,'excon').locator('[data-response$=":police"] [data-reply-state="Pending"]').waitFor();
  },
  async 'excon-suggest'(page){
    const excon=col(page,'excon');
    await openResponse(page,'mayor');
    await excon.locator('[data-suggest-actions]').click();await done(page,1);
    await excon.locator('[data-add-excon-suggestions]').click();
    const approve=excon.locator('[data-approve]');
    await excon.locator('.verdict-ok').first().waitFor();await done(page,2);
    await assert.doesNotReject(approve.click());await done(page,3);
    await excon.locator('.response-dialog').getByRole('button',{name:/Chiudi|Close/}).first().click();
    await openInject(page,'mayor');
    await col(page,'mayor').locator('.inj-msg.proposal').filter({hasText:/Approvata|Approved/}).waitFor();
  },
  async clarify(page){
    const excon=col(page,'excon'),mayor=col(page,'mayor');
    await openResponse(page,'mayor');
    await excon.locator('[data-note]').fill('Quale zona va evacuata, e verso dove?');
    await excon.locator('[data-clarify]').click();await done(page,1);
    await openInject(page,'mayor');
    await mayor.locator('.clarify-reply textarea').fill('La zona F4, verso la palestra comunale.');
    await mayor.locator('[data-send-clarification]').click();await done(page,2);
    await excon.locator('[data-close-clarification]').click();
  },
  async conversation(page){
    const mayor=col(page,'mayor'),police=col(page,'police'),excon=col(page,'excon');
    await openInject(page,'mayor');await mayor.locator('[data-reply-inject]').click();await mayor.locator('[data-reply-conversation]').click();
    await mayor.locator('[data-conv-person="police"]').check();
    await mayor.locator('#conv-first').fill('Riuscite a chiudere la SP12 prima di evacuare F4?');
    await mayor.locator('[data-create-conversation]').click();await done(page,1);
    await openInject(page,'police');await police.locator('[data-conv-row]').click();
    await police.locator('#conv-text').fill('Sì, pattuglia in posizione tra 10 minuti.');
    await police.locator('[data-send-conversation]').click();await done(page,2);
    await mayor.locator('[data-conversation] [data-conv-message]').nth(1).waitFor();
    await mayor.locator('[data-respond-from-conversation]').click();
    await mayor.locator('#response-narrative').fill('D’accordo con la Polizia: SP12 chiusa, poi evacuiamo F4.');
    await mayor.locator('#kind-decision').check();await mayor.locator('#basis-operational_judgment').check();
    await mayor.locator('[data-respond]').click();await done(page,3);
    await openResponse(page,'mayor');
    await excon.locator('[data-linked-conversation]').first().waitFor();
    await excon.locator('[data-approve]').click();
  },
  async decide(page){
    const excon=col(page,'excon');
    await openResponse(page,'police');
    // Partial approval: the second action is left out with a reason.
    await excon.locator('[data-approve-action="1"]').uncheck();
    await excon.locator('[data-reject-reason="1"]').fill('La zona E3 non è minacciata.');
    await excon.locator('[data-approve]').click();await done(page,2);
    await excon.locator('.response-dialog').getByRole('button',{name:/Chiudi|Close/}).first().click();
    await openInject(page,'police');
    await col(page,'police').locator('.inj-actions .not-approved').waitFor();
  },
};

(async()=>{
  fs.mkdirSync(out,{recursive:true});
  const browser=await chromium.launch({headless:true,...(process.env.TTX_BROWSER_EXECUTABLE?{executablePath:process.env.TTX_BROWSER_EXECUTABLE}:{channel:process.env.TTX_BROWSER_CHANNEL||'chrome'})});
  const page=await browser.newPage({viewport:{width:1680,height:1000}}),errors=[];
  page.on('pageerror',e=>errors.push(e.message));
  try{
    for(const [id,run] of Object.entries(benches)){
      await page.goto(`${base}?bench=1&b=${id}`);
      await page.locator(`[data-bench="${id}"]`).waitFor({timeout:60000});
      assert.equal(await page.locator('[data-bench-step][data-status="active"]').count(),1,`${id}: one active step`);
      await run(page);
      await allDone(page,id);
      await page.screenshot({path:path.join(out,`${id}.png`)});
      // Reset brings the bench back to its first steps.
      await page.locator('[data-bench-reset]').click();
      assert.equal(await page.locator('[data-bench-step][data-status="active"]').count(),1,`${id}: reset`);
    }
    // English console text through the EXCON language switch.
    await page.goto(`${base}?bench=1&b=inject`);
    await page.locator('[data-bench-lang="en"]').click();
    await page.getByRole('heading',{name:'Test bench'}).waitFor();
    await page.locator('[data-bench-lang="it"]').click();
    await col(page,'excon').locator('[data-bench-send="I1"]').waitFor();
    // Overview: the clarification bench half-way (done, active and pending steps).
    await page.goto(`${base}?bench=1&b=clarify`);
    await openResponse(page,'mayor');
    await col(page,'excon').locator('[data-note]').fill('Quale zona va evacuata, e verso dove?');
    await col(page,'excon').locator('[data-clarify]').click();await done(page,1);
    await openInject(page,'mayor');
    await col(page,'mayor').locator('.clarify-reply textarea').fill('La zona F4, verso la palestra comunale.');
    await page.screenshot({path:process.env.TTX_BENCH_SHOT||path.join(out,'bench.png')});
    assert.deepEqual(errors,[],'no page errors');
    console.log(`bench QA ok · screenshots in ${out}`);
  }catch(e){
    await page.screenshot({path:path.join(out,'failure.png')}).catch(()=>{});
    console.error(e);process.exitCode=1;
  }finally{await browser.close();}
})();

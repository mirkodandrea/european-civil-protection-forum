// Player reply flow (30/10 playtest): one composer, one Send, recipient always
// visible. A reply to the inject's sender requires its basis (plan or
// procedure, law or regulation, operational judgement), the reference of a
// plan or law, and the rationale; Send stays disabled with a hint until then.
// Starting a conversation with other players needs the same: a basis (plan or
// procedure, law or regulation, personal initiative), the reference of a plan
// or law and why; the participants read it as the opening context. Later
// messages inside the conversation need none of it.
// Drives the workflow bench (standalone+bench build, simulated server) and
// takes one screenshot per state:
//   PORT=8085 ./scripts/dev_bench.sh
//   TTX_BENCH_URL=http://127.0.0.1:8085/ node scripts/qa/player-reply-flow.cjs
// TTX_QA_OUT: screenshot directory (default runs/player-reply-flow/<time>).
const {chromium}=require(process.env.TTX_PLAYWRIGHT||'playwright');
const fs=require('fs'),path=require('path'),assert=require('assert/strict');

const base=(process.env.TTX_BENCH_URL||'http://127.0.0.1:8080/').replace(/\?.*$/,'');
const out=process.env.TTX_QA_OUT||path.join('runs/player-reply-flow',new Date().toISOString().replace(/[:.]/g,'-'));
const col=(page,id)=>page.locator(`[data-bench-col="${id}"]`);

(async()=>{
  fs.mkdirSync(out,{recursive:true});
  const browser=await chromium.launch({headless:true,...(process.env.TTX_BROWSER_EXECUTABLE?{executablePath:process.env.TTX_BROWSER_EXECUTABLE}:{channel:process.env.TTX_BROWSER_CHANNEL||'chrome'})});
  const page=await browser.newPage({viewport:{width:1680,height:1100}}),errors=[];
  page.on('pageerror',e=>errors.push(e.message));
  const shot=async name=>{await page.screenshot({path:path.join(out,`${name}.png`)});console.log(`  ${name}.png`);};
  try{
    await page.goto(`${base}?bench=1&b=conversation`);
    await page.locator('[data-bench="conversation"]').waitFor({timeout:60000});
    const mayor=col(page,'mayor'),police=col(page,'police'),excon=col(page,'excon');
    await mayor.locator('[data-inject-row]').first().click();
    const box=mayor.locator('[data-reply-composer]');
    // One box, the sender preselected and named, one Send naming the recipient.
    await box.locator('[data-to="sender"]:checked').waitFor();
    assert.equal(await mayor.locator('[data-reply-send]').count(),1,'one Send');
    for(const gone of ['[data-reply-direct]','[data-reply-conversation]','[data-respond-from-conversation]','[data-create-conversation]','.work-modal'])
      assert.equal(await page.locator(gone).count(),0,`no ${gone}`);
    const send=mayor.locator('[data-reply-send]'),hint=mayor.locator('.reply-send-bar .work-hint');
    const blocked=async(re,why)=>{assert.equal(await send.isDisabled(),true,`Send disabled: ${why}`);assert.match(await hint.innerText(),re,why);};
    await mayor.locator('#response-narrative').fill('Apriamo la palestra comunale come area di attesa per i residenti di F4.');
    await blocked(/fondamento|si basa|rests on/i,'basis required');
    await shot('1a-sender-needs-basis');
    await mayor.locator('#basis-plan').click();
    await blocked(/quale piano|which plan/i,'plan reference required');
    await mayor.locator('[data-basis-note]').fill('Piano comunale di protezione civile, scheda 4 (aree di attesa)');
    await blocked(/motivazione|rationale/i,'rationale required');
    await mayor.locator('[data-rationale]').fill('Il fronte può raggiungere F4 entro un’ora: i residenti devono attendere al coperto.');
    assert.equal(await send.isDisabled(),false,'Send enabled once everything is filled');
    assert.match(await send.innerText(),/Invia a|Send to/);
    await shot('1-inject-reply-to-sender');
    // Same box, recipient = other players: it starts a conversation, so it
    // needs its reason (basis, reference of a plan or law, why).
    await box.locator('[data-to="players"]').check();
    await mayor.locator('[data-conv-person="police"]').check();
    await mayor.locator('#response-narrative').fill('Riuscite a chiudere la SP12 prima di evacuare F4?');
    await mayor.locator('[data-reason-for="conversation"]').waitFor();
    assert.match(await mayor.locator('#basis-operational_judgment').innerText(),/Iniziativa personale|Personal initiative/,'third basis: personal initiative');
    // The basis picked for the sender carries over: clear it to see the hint.
    if(await mayor.locator('#basis-plan[aria-pressed="true"]').count())await mayor.locator('#basis-plan').click();
    await mayor.locator('[data-rationale]').fill('');
    await blocked(/su cosa si basa la conversazione|conversation rests on/i,'conversation basis required');
    await mayor.locator('#basis-law').click();
    await mayor.locator('[data-basis-note]').fill('');
    await blocked(/quale piano|which plan/i,'law reference required');
    await mayor.locator('[data-basis-note]').fill('D.Lgs. 1/2018, art. 12 (funzioni dei Comuni)');
    await blocked(/perché avvii|why you start/i,'conversation rationale required');
    await mayor.locator('[data-rationale]').fill('Prima di evacuare F4 serve la strada libera: la chiusura della SP12 spetta alla Polizia.');
    assert.equal(await send.isDisabled(),false,'Send enabled once the reason is there');
    await shot('2-inject-to-players');
    await mayor.locator('[data-send-conversation]').click();
    await mayor.locator('[data-conversation]').waitFor();
    // The conversation opens with its reason.
    assert.match(await mayor.locator('[data-conversation] [data-conv-reason]').innerText(),/serve la strada libera/);
    // Inside the conversation: participants preselected, no reason asked.
    await police.locator('[data-inject-row]').first().click();await police.locator('[data-conv-row]').click();
    await police.locator('[data-reply-composer][data-reply-to="players"]').waitFor();
    const policeReason=await police.locator('[data-conversation] [data-conv-reason]').innerText();
    assert.match(policeReason,/D\.Lgs\. 1\/2018/,'participants read the reference');
    assert.match(policeReason,/serve la strada libera/,'participants read why');
    assert.equal(await police.locator('[data-reply-composer] [data-basis]').count(),0,'no basis inside a conversation');
    assert.equal(await police.locator('[data-reply-composer] [data-rationale]').count(),0,'no rationale inside a conversation');
    await police.locator('#response-narrative').fill('Sì, pattuglia in posizione tra 10 minuti.');
    await police.locator('[data-send-conversation]').click();
    await mayor.locator('[data-conversation] [data-conv-message]').nth(1).waitFor();
    await shot('3-conversation-to-players');
    // From the conversation, the reply to the inject: switch the recipient, one Send.
    await mayor.locator('[data-reply-composer] [data-to="sender"]').check();
    await mayor.locator('#response-narrative').fill('D’accordo con la Polizia: SP12 chiusa, poi evacuiamo F4 verso la palestra.');
    await mayor.locator('#basis-operational_judgment').click();
    assert.equal(await mayor.locator('[data-basis-note]').count(),0,'no reference line for operational judgement');
    await blocked(/motivazione|rationale/i,'rationale required');
    await mayor.locator('[data-rationale]').fill('Il fronte arriva in F4 entro un’ora: prima liberiamo la strada');
    await shot('4-conversation-to-sender');
    await mayor.locator('[data-respond]').click();
    await mayor.locator('[data-conv-response]').first().waitFor();
    // EXCON sees the basis and the player's rationale.
    await excon.locator('[data-response$=":mayor"] .response-row').first().click();
    await excon.locator('[data-reply-rationale]').first().waitFor();
    assert.match(await excon.locator('[data-reply-rationale]').first().innerText(),/liberiamo la strada/);
    await shot('5-excon-sees-basis');
    assert.deepEqual(errors,[],'no page errors');
    console.log(`player reply flow ok · screenshots in ${out}`);
  }catch(e){
    await page.screenshot({path:path.join(out,'failure.png')}).catch(()=>{});
    console.error(e);process.exitCode=1;
  }finally{await browser.close();}
})();

// Full-standalone QA of starting a conversation (2026-10-30, reworked after
// the 02/11 playtest): the real app (exercise model and server routes in
// WASM, not the bench's fake), the AI intercepted locally (translations echo
// the text, prefixed with the target language when it is not Italian or English).
// 1. The Mayor receives an inject and starts a conversation with the DOS
//    from the inject's box: Send stays disabled until the basis (plan or
//    procedure, law or regulation, personal initiative), the reference of a
//    plan or law and the reason ("why you start it", read by EXCON only)
//    are there. Participants read one opening message, no reason block.
//    The inject's thread and the conversation are sibling tabs.
// 2. The DOS joins late: the exercise started before the DOS confirmed a
//    language. Picking French translates what already reached the DOS. The
//    DOS never received the inject: the row opens the conversation directly,
//    the quote shows the title only, with no note about it, and no reason.
// 3. EXCON reads the reason next to the conversation's title in its viewer
//    (and the full inject).
// Screenshots go to TTX_QA_OUT (default runs/conversation-start/<time>);
// TTX_QA_VIEWPORT (default 1600x1000) sets the page size.
//   TTX_QA_URL=http://127.0.0.1:8096/ node scripts/qa/conversation-start.cjs
const { chromium } = require(process.env.TTX_PLAYWRIGHT || 'playwright');
const assert = require('assert/strict');
const fs = require('fs');
const path = require('path');
const root = path.resolve(__dirname, '../..');
const out = path.resolve(process.env.TTX_QA_OUT || path.join(root, 'runs/conversation-start', new Date().toISOString().replace(/[:.]/g, '-')));
const url = process.env.TTX_QA_URL || 'http://127.0.0.1:8080/';

const TITLE = 'Albero sulla SP12';
const INJECT = 'Un pino è caduto sulla SP12 tra E4 e F4: la strada è bloccata in entrambe le direzioni';
const FIRST = 'Riuscite a mandare una squadra a liberare la SP12 prima dell’evacuazione di F4?';
const REFERENCE = 'Piano comunale di protezione civile, scheda 3 (viabilità di emergenza)';
const WHY = 'La SP12 è l’unica via di fuga da F4: senza i VVF non possiamo evacuare in sicurezza';

(async () => {
  fs.mkdirSync(out, { recursive: true });
  const errors = [], tasks = [];
  const browser = await chromium.launch({ headless: true, ...(process.env.TTX_BROWSER_EXECUTABLE ? { executablePath: process.env.TTX_BROWSER_EXECUTABLE } : { channel: process.env.TTX_BROWSER_CHANNEL || 'chrome' }) });
  // TTX_QA_VIEWPORT=1366x680 checks a laptop screen (02/11 playtest).
  const [vw, vh] = (process.env.TTX_QA_VIEWPORT || '1600x1000').split('x').map(Number);
  const page = await browser.newPage({ viewport: { width: vw, height: vh }, deviceScaleFactor: 1 });
  page.on('pageerror', e => { if (!e.message.includes('control flow')) errors.push(e.message); });
  // Local AI: translations return the text (marked with a non-Italian, non-English target); nothing else is expected.
  await page.route('https://openrouter.ai/api/v1/chat/completions', async route => {
    const req = route.request().postDataJSON();
    const system = req.messages?.[0]?.content || '';
    const user = req.messages?.at(-1)?.content || '';
    tasks.push(system.slice(0, 40));
    const target = (system.match(/literally into (\w+)/) || [])[1];
    const content = system.startsWith('Translate') ? (target && !['Italian', 'English'].includes(target) ? `[${target}] ${user}` : user) : JSON.stringify({ actions: [], questions: [], summary: '' });
    await route.fulfill({ status: 200, contentType: 'application/json', headers: { 'Access-Control-Allow-Origin': '*' },
      body: JSON.stringify({ choices: [{ message: { content } }] }) });
  });
  const panel = role => page.locator(`[data-panel="${role}"]`);
  const choose = role => page.locator(`.role-tabs [data-role="${role}"]`).click();
  const master = panel('master'), fire = panel('fire_commander'), mayor = panel('mayor');
  const shot = async name => { await page.waitForTimeout(300); await page.screenshot({ path: path.join(out, name + '.png') }); console.log(`  ${name}.png`); };
  const openInject = async (p, role) => {
    await choose(role);
    await p.locator('[data-view="map"]').click();
    const back = p.locator('[data-inject-back]');
    if (await back.count()) await back.click();
    await p.locator('[data-inject-row]').first().click();
  };
  try {
    await page.goto(url, { waitUntil: 'domcontentloaded' });
    await page.evaluate(() => { localStorage.clear(); sessionStorage.clear(); });
    await page.goto(url, { waitUntil: 'domcontentloaded' });
    await page.locator('.launcher').waitFor({ timeout: 90000 });
    await page.locator('#launch-openrouter-key').fill('qa-fixture-only');
    await page.locator('#launch-save-key').click();
    await page.locator('.launcher #start-session').click();
    await master.locator('#start').waitFor({ timeout: 60000 });
    const sim = page.getByLabel('Simulazione', { exact: true }); if (await sim.count() && await sim.isChecked()) await sim.uncheck();
    // The DOS has not picked a language yet: EXCON starts anyway.
    await require('./standalone-languages.cjs').confirmLanguages(page, { fire_commander: null, mayor: 'it' });
    await master.locator('#start-anyway').click();
    await master.locator('#turn').filter({ hasText: /orders|ordini/ }).waitFor();

    // EXCON sends the inject to the Mayor only.
    await master.locator('[data-view="timeline"]').click();
    await master.locator('[data-inject-all]').click();
    const modal = page.getByRole('dialog');
    await modal.locator('#inject-title').fill(TITLE);
    await modal.locator('#inject-sender').fill('Sala operativa');
    await modal.locator('#text').fill(INJECT);
    await modal.locator('#inject-scope').selectOption('custom');
    await modal.getByLabel(/Sindaco|Mayor/).check();
    await modal.locator('#send').click();
    await modal.waitFor({ state: 'hidden' });

    // 1. The Mayor starts a conversation with the DOS: basis and reference are required, why goes in the message.
    await openInject(mayor, 'mayor');
    const box = mayor.locator('[data-reply-composer]');
    await box.locator('[data-to="players"]').check();
    await mayor.locator('[data-conv-person="fire_commander"]').check();
    await mayor.locator('#response-narrative').fill(FIRST);
    const send = mayor.locator('[data-send-conversation]'), hint = mayor.locator('.reply-send-bar .work-hint');
    const blocked = async (re, why) => { assert.equal(await send.isDisabled(), true, `Send disabled: ${why}`); assert.match(await hint.innerText(), re, why); };
    await mayor.locator('[data-reason-for="conversation"]').waitFor();
    assert.match(await mayor.locator('#basis-operational_judgment').innerText(), /Iniziativa personale|Personal initiative/);
    await blocked(/su cosa si basa la conversazione|conversation rests on/i, 'basis required');
    await shot('1-start-needs-reason');
    await mayor.locator('#basis-plan').click();
    await blocked(/quale piano|which plan/i, 'plan reference required');
    await mayor.locator('[data-basis-note]').fill(REFERENCE);
    await blocked(/perché avvii|why you start/i, 'reason required');
    await mayor.locator('[data-rationale]').fill(WHY);
    assert.equal(await send.isDisabled(), false, 'Send enabled with basis, reference and reason');
    await shot('2-start-ready');
    await send.click();
    // The new conversation opens as a tab next to the inject's thread: one opening message with its basis.
    const opening = mayor.locator('[data-conversation] [data-conv-message]');
    await opening.first().waitFor();
    assert.equal(await mayor.locator('[data-inj-tab][aria-selected="true"]').getAttribute('data-inj-tab') !== 'thread', true, 'the conversation tab is selected');
    assert.equal(await mayor.locator('[data-inj-tab="thread"]').count(), 1, 'the thread with the sender is a sibling tab');
    assert.equal(await mayor.locator('[data-conv-reason], .conv-head, .inj-back:not([data-inject-back])').count(), 0, 'no reason card, no second back button');
    assert.equal(await opening.count(), 1, 'one message, not two');
    assert.ok(!(await mayor.locator('[data-conversation]').innerText()).includes('unica via di fuga'), 'the reason is not in the participants\' view');
    assert.equal(await mayor.locator('[data-conv-basis], [data-conv-reason]').count(), 0, 'no reason line for players');
    assert.equal(await opening.first().locator('[data-conv-cites]').count(), 0, 'the opening message does not cite the inject quoted above');
    assert.match(await mayor.locator('[data-conversation] [data-conv-quote]').innerText(), /pino è caduto/, 'the recipient keeps the text');
    await shot('3-mayor-conversation');
    await mayor.locator('[data-inj-tab="thread"]').click();
    await mayor.locator('.inj-chat .inj-msg.inject').filter({ hasText: 'pino è caduto' }).waitFor();
    assert.equal(await mayor.locator('[data-conversation]').count(), 0, 'the thread tab shows the thread');
    await shot('3b-mayor-thread-tab');
    await mayor.locator('[data-inj-tab]:not([data-inj-tab="thread"])').first().click();
    await mayor.locator('[data-conversation]').waitFor();

    // 2. The DOS joins late and picks French: what reached them is translated now. Title only, no note.
    await choose('fire_commander');
    const gate = fire.locator('#language-gate');
    await gate.waitFor();
    await gate.locator('[data-lang="fr"]').click();
    await gate.waitFor({ state: 'detached' });
    await fire.locator('[data-view="map"]').click();
    const row = fire.locator('[data-inject-row]').first();
    await row.waitFor();
    assert.match(await row.innerText(), new RegExp(TITLE));
    assert.ok(!(await fire.innerText()).includes('pino è caduto'), 'no inject text in the guest list');
    await row.locator('.inj-last').filter({ hasText: '[French]' }).waitFor({ timeout: 15000 });
    await shot('4-guest-row');
    await row.click();
    // A guest has no thread of its own: the conversation opens directly, without tabs to switch.
    await fire.locator('[data-conversation] [data-conv-quote][data-title-only]').waitFor();
    assert.equal(await fire.locator('[data-inj-tab="thread"], [data-open-guest-conversation]').count(), 0, 'no thread tab, no extra button');
    assert.equal(await fire.locator('.conv-quote-note, [data-conv-reason]').count(), 0, 'no "title only" note, no reason card');
    assert.equal(await fire.locator('[data-conversation] [data-conv-quote] p').count(), 0, 'the quote has no body');
    const first = fire.locator('[data-conversation] [data-conv-message]').first();
    assert.match(await first.innerText(), /\[French\] Riuscite/, 'the opening message, translated after the late language pick');
    const guestText = await fire.innerText();
    assert.ok(!guestText.includes('unica via di fuga') && !guestText.includes('scheda 3'), 'the guest never reads the reason');
    // For players EXCON does not exist: no player page names it.
    for (const [who, p] of [['guest', fire], ['author', mayor]]) assert.ok(!/EXCON|Exercise control/.test(await p.innerText()), `${who} page names EXCON`);
    assert.ok(!(await fire.innerText()).includes('pino è caduto'), 'no inject text in the guest conversation');
    assert.equal(await fire.locator('[data-reply-composer] [data-basis]').count(), 0, 'no basis inside the conversation');
    await shot('5-guest-conversation');
    await fire.locator('#response-narrative').fill('Mandiamo una squadra con la motosega: SP12 libera in 20 minuti.');
    await fire.locator('[data-send-conversation]').click();
    await fire.locator('[data-conversation] [data-conv-message]').nth(1).waitFor();
    await shot('6-guest-reply');

    // 3. EXCON reads the reason next to the conversation's title in the viewer.
    await choose('master');
    await master.locator('[data-view="risposte"]').click();
    await master.locator('[data-conv-badge]').first().click();
    await master.locator('.conv-viewer .conv-head [data-conv-reason]').waitFor();
    assert.match(await master.locator('.conv-viewer [data-conv-reason]').innerText(), /scheda 3[\s\S]*unica via di fuga/);
    assert.equal(await master.locator('.conv-viewer [data-conv-message]').first().locator('[data-conv-basis]').count(), 0, 'not inside the first message');
    assert.match(await master.locator('.conv-viewer').innerText(), /pino è caduto/, 'EXCON reads the inject text');
    await shot('7-excon-viewer');
    assert.deepEqual(errors, [], 'no page errors');
    console.log(`conversation start ok · AI calls: ${tasks.length} · screenshots in ${out}`);
  } catch (e) {
    await page.screenshot({ path: path.join(out, 'failure.png') }).catch(() => {});
    console.error(e); process.exitCode = 1;
  } finally { await browser.close(); }
})();

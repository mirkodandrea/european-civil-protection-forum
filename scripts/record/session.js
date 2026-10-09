// Plays a two-turn TTX session with every role in one phone-sized window
// (play.html), recording the page to video. Captions explain each step.
// Usage: node session.js <base_url> <out_dir> <rec_flag_file>
const { chromium } = require('playwright');
const fs = require('fs');
const path = require('path');

const BASE = process.argv[2] || 'http://localhost:8090';
const OUT = process.argv[3] || './out';
const REC_FLAG = process.argv[4];
// Narration with durations, written by tts.js.
const NARRATION = new Map(JSON.parse(require('fs').readFileSync(process.argv[5] || path.join(OUT, 'narration.json'), 'utf8')).lines.map(l => [l.id, l]));
const sleep = ms => new Promise(r => setTimeout(r, ms));
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);

async function getJSON(p) { return (await fetch(BASE + p)).json(); }
async function waitFor(pred, label, timeout = 90000) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeout) {
    try { if (await pred()) return; } catch {}
    await sleep(500);
  }
  throw new Error('timeout waiting for ' + label);
}
const noneParsing = async () => !(await getJSON('/api/master')).proposals.some(p => p.status === 'parsing');

(async () => {
  fs.mkdirSync(OUT, { recursive: true });
  const browser = await chromium.launch({ channel: 'chrome' });
  const context = await browser.newContext({
    viewport: { width: 480, height: 1040 }, deviceScaleFactor: 1, isMobile: true, hasTouch: true,
    recordVideo: { dir: OUT, size: { width: 480, height: 1040 } },
  });
  const page = await context.newPage();
  const videoStart = Date.now();
  if (REC_FLAG) fs.writeFileSync(REC_FLAG, '');
  fs.writeFileSync(path.join(OUT, 'video_start_ms.txt'), String(videoStart));
  const marks = [];

  await page.goto(BASE + '/play.html');
  await page.waitForSelector('#bar button');
  await sleep(1500);

  // Narration: each line is a WAV of known length (tts.js). A caption marks
  // the moment its line starts; the next one waits until the previous has been
  // spoken, so the voice paces the session. compose.py lays the audio and the
  // subtitles on these marks.
  let speechEnds = 0;
  const caption = async (id, hold = 0) => {
    const line = NARRATION.get(id);
    if (!line) throw new Error('no narration line ' + id);
    const wait = speechEnds - Date.now();
    if (wait > 0) await sleep(wait);
    const t = Date.now() - videoStart;
    log('SAY', id, line.duration_s.toFixed(1) + 's');
    marks.push({ id, t, duration_s: line.duration_s, text: line.text, caption: line.text });
    speechEnds = Date.now() + line.duration_s * 1000 + 350;
    if (hold) await sleep(hold);
  };
  const tab = async name => {
    await page.locator('#bar button', { hasText: name }).first().click();
    await sleep(700);
  };
  const F = name => page.frameLocator(`iframe[title="${name}"]`);
  const type = async (loc, text, delay = 18) => { await loc.click(); await loc.pressSequentially(text, { delay }); };
  const scrollInbox = async (name, where = 'bottom') => {
    await F(name).locator('#inbox').evaluate((el, w) => el.scrollTo({ top: w === 'bottom' ? el.scrollHeight : 0, behavior: 'smooth' }), where);
    await sleep(900);
  };
  const scrollTo = async (name, selector) => {
    await F(name).locator(selector).first().scrollIntoViewIfNeeded();
    await sleep(600);
  };
  const lastPid = async () => (await getJSON('/api/master')).proposals.at(-1).id;

  // ---------------------------------------------------------------- turn 1
  await caption('intro', 1500);
  await tab('Regional');
  await caption('language', 1000);
  await F('Regional').locator('#joinlang').selectOption('hy');
  await F('Regional').locator('#confirmlang').click();
  await tab('Mayor');
  await F('Mayor').locator('#joinlang').selectOption('it');
  await F('Mayor').locator('#confirmlang').click();
  await tab('Fire');
  await F('Fire').locator('#joinlang').selectOption('en');
  await F('Fire').locator('#confirmlang').click();
  await sleep(1800);

  await tab('Director');
  await caption('start', 1200);
  await F('Director').locator('#start').click();
  // Turns are simulated live on the big screen: 120× = one simulated minute every half second.
  await F('Director').locator('#speed').fill('120');
  await F('Director').locator('#speed').dispatchEvent('change');
  await sleep(2000);
  await caption('injects', 500);
  await F('Director').locator('#tb-injects').click();
  await sleep(800);
  for (const id of ['I-01', 'I-02', 'I-03']) {
    await scrollTo('Director', `[data-msel="${id}"]`);
    await F('Director').locator(`[data-msel="${id}"]`).click();
    await waitFor(async () => (await getJSON('/api/state')).sent_injects.includes(id), id);
    await sleep(600);
  }

  await tab('Mayor');
  await caption('mayor_reads', 500);
  await scrollInbox('Mayor', 'top');
  await sleep(3500);
  await caption('mayor_replies', 300);
  await scrollTo('Mayor', '#reply');
  await type(F('Mayor').locator('#reply'), "Allertate la zona F4 ed evacuate la scuola primaria di Pineta verso il centro sportivo di Valverde, da aprire come area di accoglienza. Chiudete la Northern Road al traffico civile. Avvisate i vigili del fuoco.");
  await F('Mayor').locator('#basiskind').selectOption('operational_judgment');
  await F('Mayor').locator('#basis').fill('Operational judgment based on the received situation and available units.');
  await F('Mayor').locator('#replyto').selectOption(await F('Mayor').locator('#replyto option').last().getAttribute('value'));
  await F('Mayor').locator('#send').click();
  await sleep(1200);

  await tab('Fire');
  await caption('fire_orders', 300);
  await scrollInbox('Fire', 'bottom');
  await sleep(1500);
  await scrollTo('Fire', '#reply');
  await type(F('Fire').locator('#reply'), 'Two crews cut a fire line along the northern edge of zone F4; the third crew from Pineta east edge to Forest edge east. Air tanker: one drop on Pineta north edge. Tell regional civil protection we may need reinforcements.');
  await F('Fire').locator('#basiskind').selectOption('operational_judgment');
  await F('Fire').locator('#basis').fill('Operational judgment based on the received situation and available units.');
  await F('Fire').locator('#replyto').selectOption(await F('Fire').locator('#replyto option').last().getAttribute('value'));
  await F('Fire').locator('#send').click();
  await sleep(1000);

  await tab('Director');
  await caption('proposals', 300);
  await F('Director').locator('#tb-proposals').click();
  await waitFor(noneParsing, 'turn-1 parsing');
  await sleep(1500);
  const m1 = await getJSON('/api/master');
  const mayorP = m1.proposals.find(p => p.source.role === 'mayor').id;
  const fireP = m1.proposals.find(p => p.source.role === 'fire_commander').id;
  await scrollTo('Director', `[data-approve="${mayorP}"]`);
  await caption('buses', 1500);

  await caption('prompt', 300);
  await F('Director').locator('#tb-prompt').click();
  await sleep(600);
  await type(F('Director').locator('#ptext'), 'The regional transport company gives the mayor one extra bus right now. Send the mayor a short inject from the Regional transport company confirming it.', 12);
  await F('Director').locator('#psend').click();
  await sleep(1500);
  await waitFor(noneParsing, 'director prompt');
  const dirP = await lastPid();
  await sleep(1000);
  await scrollTo('Director', `[data-approve="${dirP}"]`);
  await caption('prompt_approve', 1500);
  await F('Director').locator(`[data-approve="${dirP}"]`).click();
  await sleep(2500);
  await scrollTo('Director', `[data-approve="${fireP}"]`);
  await caption('fire_approve', 300);
  await type(F('Director').locator(`#note-${fireP}`), 'Approved. F4 north edge first.', 25);
  await F('Director').locator(`[data-approve="${fireP}"]`).click();
  await sleep(2000);
  await scrollTo('Director', `[data-approve="${mayorP}"]`);
  await caption('mayor_approve', 1500);
  await F('Director').locator(`[data-approve="${mayorP}"]`).click();
  await sleep(1500);

  await tab('Mayor');
  await caption('mayor_feedback', 300);
  await waitFor(async () => (await getJSON('/api/player/mayor')).inbox.some(m => m.kind === 'feedback'), 'mayor feedback');
  await sleep(1200);
  await scrollInbox('Mayor', 'bottom');
  await sleep(4000);

  await tab('Director');
  await caption('resolve1', 300);
  await F('Director').locator('#resolve').scrollIntoViewIfNeeded();
  await F('Director').locator('#resolve').click();
  await sleep(3000);
  await caption('flames', 300);
  await sleep(6000);
  await caption('turn1_result', 300);
  await waitFor(async () => (await getJSON('/api/state')).phase === 'orders', 'turn 1 resolved', 120000);
  await sleep(1500);

  await tab('Fire');
  await caption('sitrep', 300);
  await waitFor(async () => (await getJSON('/api/player/fire_commander')).inbox.some(m => m.kind === 'situation'), 'sitrep');
  await sleep(800);
  await scrollInbox('Fire', 'bottom');
  await sleep(4500);

  // ---------------------------------------------------------------- turn 2
  await tab('Director');
  await caption('turn2', 300);
  await F('Director').locator('#tb-injects').click();
  await sleep(500);
  await scrollTo('Director', '[data-msel="I-04"]');
  await F('Director').locator('[data-msel="I-04"]').click();
  await waitFor(async () => (await getJSON('/api/state')).sent_injects.includes('I-04'), 'I-04');
  await F('Director').locator('#tb-commands').click();
  await sleep(500);
  await F('Director').locator('#ctext').scrollIntoViewIfNeeded();
  await F('Director').locator('#ctext').fill('');
  const returnedUnits = (await getJSON('/api/state')).world.allocations.filter(a => a.role === 'fire_commander' && a.qty > a.returned).map(a => ({type:'ReturnResource', allocation:a.id, qty:a.qty-a.returned}));
  // EXCON explicitly confirms first-turn fire tasks completed before reassigning units.
  await type(F('Director').locator('#ctext'), JSON.stringify([...returnedUnits, {type:'SetWeather', wind_speed_kmh:55}, {type:'Ignite', at:'G5', radius_m:60}], null, 1), 20);
  await F('Director').locator('#csend').click();
  await sleep(3000);

  await tab('Regional');
  await caption('ucpm', 300);
  await scrollInbox('Regional', 'bottom');
  await sleep(2000);
  await scrollTo('Regional', '#reply');
  await type(F('Regional').locator('#reply'), 'Խնդրում ենք միջազգային օգնությամբ (UCPM) 2 օդային տանկեր և տարածաշրջանային հրշեջ ծառայությունից 2 վերգետնյա թիմ հրշեջ հրամանատարի համար։ Տեղեկացրեք քաղաքապետին, որ Valverde Sports Centre-ը բաց է։', 10);
  await F('Regional').locator('#basiskind').selectOption('operational_judgment');
  await F('Regional').locator('#basis').fill('Operational judgment based on the received situation and available units.');
  await F('Regional').locator('#replyto').selectOption(await F('Regional').locator('#replyto option').last().getAttribute('value'));
  await F('Regional').locator('#send').click();
  await sleep(1000);

  await tab('Fire');
  await caption('vague', 300);
  await scrollTo('Fire', '#reply');
  await type(F('Fire').locator('#reply'), 'Send the remaining crews over there to hold it.', 30);
  await F('Fire').locator('#basiskind').selectOption('operational_judgment');
  await F('Fire').locator('#basis').fill('Operational judgment based on the received situation and available units.');
  await F('Fire').locator('#replyto').selectOption(await F('Fire').locator('#replyto option').last().getAttribute('value'));
  await F('Fire').locator('#send').click();
  await waitFor(noneParsing, 'clarification draft');
  const clarificationDraft = (await getJSON('/api/master')).proposals.find(p => p.status === 'needs_clarification' && p.source.role === 'fire_commander');
  if (!clarificationDraft) throw new Error('No clarification draft to review');
  await tab('Director');
  await F('Director').locator('#tb-proposals').click();
  await scrollTo('Director', `[data-approve="${clarificationDraft.id}"]`);
  await F('Director').locator(`[data-approve="${clarificationDraft.id}"]`).click();
  await tab('Fire');
  await waitFor(async () => (await getJSON('/api/player/fire_commander')).inbox.some(m => m.kind === 'clarification'), 'clarification');
  await sleep(800);
  await scrollInbox('Fire', 'bottom');
  await caption('clarify', 1500);
  await caption('precise', 300);
  await scrollTo('Fire', '#reply');
  await type(F('Fire').locator('#reply'), 'One crew along the east edge of zone F4, the other along the north edge of G4. Air tanker drop on G4.', 22);
  await F('Fire').locator('#basiskind').selectOption('operational_judgment');
  await F('Fire').locator('#basis').fill('Operational judgment based on the received situation and available units.');
  await F('Fire').locator('#replyto').selectOption(await F('Fire').locator('#replyto option').last().getAttribute('value'));
  await F('Fire').locator('#send').click();
  await sleep(800);

  await tab('Director');
  await caption('decide2', 300);
  await F('Director').locator('#tb-proposals').click();
  await waitFor(noneParsing, 'turn-2 parsing');
  await sleep(1500);
  const m2 = await getJSON('/api/master');
  const open = m2.proposals.filter(p => ['pending', 'needs_clarification'].includes(p.status));
  for (const p of open) {
    const vague = p.status === 'needs_clarification' || p.original.startsWith('Send the remaining');
    await scrollTo('Director', `[data-approve="${p.id}"]`);
    await sleep(1800);
    if (vague) {
      await type(F('Director').locator(`#note-${p.id}`), 'Please specify the zone edge.', 25);
      await F('Director').locator(`[data-reject="${p.id}"]`).click();
    } else {
      await F('Director').locator(`[data-approve="${p.id}"]`).click();
    }
    await sleep(1800);
  }

  await caption('resolve2', 300);
  await F('Director').locator('#minutes').fill('60');
  await F('Director').locator('#resolve').scrollIntoViewIfNeeded();
  await F('Director').locator('#resolve').click();
  await sleep(2000);
  await waitFor(async () => (await getJSON('/api/state')).phase === 'orders', 'turn 2 resolved', 180000);
  await sleep(2500);

  await F('Director').locator('#stats').scrollIntoViewIfNeeded();
  await caption('outro', 1500);

  const tail = speechEnds - Date.now();
  if (tail > 0) await sleep(tail + 1200);
  if (REC_FLAG) fs.rmSync(REC_FLAG, { force: true });
  fs.writeFileSync(path.join(OUT, 'marks.json'), JSON.stringify(marks, null, 1));
  const video = page.video();
  await context.close();
  await browser.close();
  const vp = await video.path();
  fs.writeFileSync(path.join(OUT, 'video_path.txt'), vp);
  log('done', vp, 'duration', ((Date.now() - videoStart) / 1000).toFixed(0) + 's');
})().catch(async e => { console.error(e); if (REC_FLAG) fs.rmSync(REC_FLAG, { force: true }); process.exit(1); });

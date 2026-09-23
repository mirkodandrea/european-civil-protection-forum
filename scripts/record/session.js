// Plays a two-turn TTX session with every role in one phone-sized window
// (play.html), recording the page to video. Captions explain each step.
// Usage: node session.js <base_url> <out_dir> <rec_flag_file>
const { chromium } = require('playwright');
const fs = require('fs');
const path = require('path');

const BASE = process.argv[2] || 'http://localhost:8090';
const OUT = process.argv[3] || './out';
const REC_FLAG = process.argv[4];
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

  // Caption bar at the bottom of the phone screen.
  await page.evaluate(() => {
    const d = document.createElement('div');
    d.id = 'cap';
    d.style.cssText = 'flex:none;min-height:96px;display:flex;align-items:center;background:#b3261e;color:#fff;font:600 17px/1.35 system-ui;padding:10px 14px;pointer-events:none';
    document.body.appendChild(d);
  });
  const caption = async (t, hold = 0) => {
    log('CAPTION', t);
    marks.push({ t: Date.now() - videoStart, caption: t });
    await page.evaluate(t => { document.getElementById('cap').textContent = t; }, t);
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
  await caption('PPRD East TTX — one phone plays every role: Director, Mayor, Fire Commander, Regional Civil Protection.', 3500);
  await tab('Regional');
  await caption('Each player picks a language. Regional Civil Protection will play in Armenian.', 1000);
  await F('Regional').locator('#lang').selectOption('hy');
  await sleep(1800);

  await tab('Director');
  await caption('The Director starts the exercise. Day 1, 13:00: a wildfire is burning on Pine Hill.', 1200);
  await F('Director').locator('#start').click();
  await sleep(2000);
  await caption('The Director sends the first injects from the MSEL. The AI translates each one for its recipient.', 500);
  await F('Director').locator('#tb-injects').click();
  await sleep(800);
  for (const id of ['I-01', 'I-02', 'I-03']) {
    await scrollTo('Director', `[data-msel="${id}"]`);
    await F('Director').locator(`[data-msel="${id}"]`).click();
    await waitFor(async () => (await getJSON('/api/state')).sent_injects.includes(id), id);
    await sleep(600);
  }

  await tab('Mayor');
  await caption('The Mayor receives the injects in Italian: the fire, and 120 pupils at Pineta Primary School.', 500);
  await scrollInbox('Mayor', 'top');
  await sleep(3500);
  await caption('The Mayor answers in Italian, in their own words.', 300);
  await scrollTo('Mayor', '#reply');
  await type(F('Mayor').locator('#reply'), "Allertate la zona F4 ed evacuate la scuola primaria di Pineta verso il centro sportivo di Valverde, da aprire come area di accoglienza. Chiudete la Northern Road al traffico civile. Avvisate i vigili del fuoco.");
  await F('Mayor').locator('#send').click();
  await sleep(1200);

  await tab('Fire');
  await caption('The Fire Commander orders fire lines using zone codes from the map grid, plus an air-tanker drop.', 300);
  await scrollInbox('Fire', 'bottom');
  await sleep(1500);
  await scrollTo('Fire', '#reply');
  await type(F('Fire').locator('#reply'), 'Two crews cut a fire line along the northern edge of zone F4; the third crew from Pineta east edge to Forest edge east. Air tanker: one drop on Pineta north edge. Tell regional civil protection we may need reinforcements.');
  await F('Fire').locator('#send').click();
  await sleep(1000);

  await tab('Director');
  await caption('The AI turns each reply into commands. The simulator checks each one against the role’s authority and resources…', 300);
  await F('Director').locator('#tb-proposals').click();
  await waitFor(noneParsing, 'turn-1 parsing');
  await sleep(1500);
  const m1 = await getJSON('/api/master');
  const mayorP = m1.proposals.find(p => p.source.role === 'mayor').id;
  const fireP = m1.proposals.find(p => p.source.role === 'fire_commander').id;
  await scrollTo('Director', `[data-approve="${mayorP}"]`);
  await caption('…and flags a real constraint: evacuating 120 pupils needs 3 buses, but the Mayor has only 2.', 5000);

  await caption('The Director writes an AI prompt: one extra bus for the Mayor, confirmed by an inject.', 300);
  await F('Director').locator('#tb-prompt').click();
  await sleep(600);
  await type(F('Director').locator('#ptext'), 'The regional transport company gives the mayor one extra bus right now. Send the mayor a short inject from the Regional transport company confirming it.', 12);
  await F('Director').locator('#psend').click();
  await sleep(1500);
  await waitFor(noneParsing, 'director prompt');
  const dirP = await lastPid();
  await sleep(1000);
  await scrollTo('Director', `[data-approve="${dirP}"]`);
  await caption('The Director reviews the AI’s proposal (a resource change plus an inject draft) and approves it.', 3000);
  await F('Director').locator(`[data-approve="${dirP}"]`).click();
  await sleep(2500);
  await scrollTo('Director', `[data-approve="${fireP}"]`);
  await caption('Now the Fire Commander’s orders are approved, with a note to the player…', 300);
  await type(F('Director').locator(`#note-${fireP}`), 'Approved. F4 north edge first.', 25);
  await F('Director').locator(`[data-approve="${fireP}"]`).click();
  await sleep(2000);
  await scrollTo('Director', `[data-approve="${mayorP}"]`);
  await caption('…and the Mayor’s orders pass this time: 3 buses for 120 pupils.', 2500);
  await F('Director').locator(`[data-approve="${mayorP}"]`).click();
  await sleep(1500);

  await tab('Mayor');
  await caption('The Mayor gets exercise control’s feedback in Italian.', 300);
  await waitFor(async () => (await getJSON('/api/player/mayor')).inbox.some(m => m.kind === 'feedback'), 'mayor feedback');
  await sleep(1200);
  await scrollInbox('Mayor', 'bottom');
  await sleep(4000);

  await tab('Director');
  await caption('The Director resolves turn 1: 30 simulated minutes. The big screen replays them.', 300);
  await F('Director').locator('#resolve').scrollIntoViewIfNeeded();
  await F('Director').locator('#resolve').click();
  await sleep(2500);
  await caption('Fire lines cut, the drop made, the school evacuated to the sports centre, the Northern Road cut by the fire.', 9000);

  await tab('Fire');
  await caption('Every player gets an AI-written situation update for their role.', 300);
  await waitFor(async () => (await getJSON('/api/player/fire_commander')).inbox.some(m => m.kind === 'situation'), 'sitrep');
  await sleep(800);
  await scrollInbox('Fire', 'bottom');
  await sleep(4500);

  // ---------------------------------------------------------------- turn 2
  await tab('Director');
  await caption('Turn 2. The Director escalates: a wind warning, then commands by code (wind 55 km/h, a new fire in zone G5).', 300);
  await F('Director').locator('#tb-injects').click();
  await sleep(500);
  await scrollTo('Director', '[data-msel="I-04"]');
  await F('Director').locator('[data-msel="I-04"]').click();
  await waitFor(async () => (await getJSON('/api/state')).sent_injects.includes('I-04'), 'I-04');
  await F('Director').locator('#tb-commands').click();
  await sleep(500);
  await F('Director').locator('#ctext').scrollIntoViewIfNeeded();
  await F('Director').locator('#ctext').fill('');
  await type(F('Director').locator('#ctext'), '[{"type":"SetWeather","wind_speed_kmh":55},\n {"type":"Ignite","at":"G5","radius_m":60}]', 20);
  await F('Director').locator('#csend').click();
  await sleep(3000);

  await tab('Regional');
  await caption('Regional Civil Protection reads the wind warning in Armenian and asks for international assistance (UCPM).', 300);
  await scrollInbox('Regional', 'bottom');
  await sleep(2000);
  await scrollTo('Regional', '#reply');
  await type(F('Regional').locator('#reply'), 'Խնդրում ենք միջազգային օգնությամբ (UCPM) 2 օդային տանկեր և տարածաշրջանային հրշեջ ծառայությունից 2 վերգետնյա թիմ հրշեջ հրամանատարի համար։ Տեղեկացրեք քաղաքապետին, որ Valverde Sports Centre-ը բաց է։', 10);
  await F('Regional').locator('#send').click();
  await sleep(1000);

  await tab('Fire');
  await caption('A vague order from the Fire Commander…', 300);
  await scrollTo('Fire', '#reply');
  await type(F('Fire').locator('#reply'), 'Send the remaining crews over there to hold it.', 30);
  await F('Fire').locator('#send').click();
  await waitFor(async () => (await getJSON('/api/player/fire_commander')).inbox.some(m => m.kind === 'clarification'), 'clarification');
  await sleep(800);
  await scrollInbox('Fire', 'bottom');
  await caption('…gets a clarification question from the AI instead of a guess.', 4500);
  await caption('The Fire Commander answers precisely.', 300);
  await scrollTo('Fire', '#reply');
  await type(F('Fire').locator('#reply'), 'One crew along the east edge of zone F4, the other along the north edge of G4. Air tanker drop on G4.', 22);
  await F('Fire').locator('#send').click();
  await sleep(800);

  await tab('Director');
  await caption('The Director rejects the vague order and approves the UCPM request and the precise fire lines.', 300);
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

  await caption('Resolve turn 2: 60 minutes. Reinforcements arrive; the lines hold above Pineta; the new fire in G5 burns farmhouses.', 300);
  await F('Director').locator('#minutes').fill('60');
  await F('Director').locator('#resolve').scrollIntoViewIfNeeded();
  await F('Director').locator('#resolve').click();
  await sleep(11000);

  await F('Director').locator('#stats').scrollIntoViewIfNeeded();
  await caption('Every inject, reply, AI interpretation, decision and consequence is logged for the debrief. Humans decide, the AI mediates.', 6000);

  if (REC_FLAG) fs.rmSync(REC_FLAG, { force: true });
  fs.writeFileSync(path.join(OUT, 'marks.json'), JSON.stringify(marks, null, 1));
  const video = page.video();
  await context.close();
  await browser.close();
  const vp = await video.path();
  fs.writeFileSync(path.join(OUT, 'video_path.txt'), vp);
  log('done', vp, 'duration', ((Date.now() - videoStart) / 1000).toFixed(0) + 's');
})().catch(async e => { console.error(e); if (REC_FLAG) fs.rmSync(REC_FLAG, { force: true }); process.exit(1); });

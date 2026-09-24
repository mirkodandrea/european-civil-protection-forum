// Generate the narration audio with OpenRouter text-to-speech (default:
// Google Gemini 3.1 Flash TTS). One WAV per narration line, cached by
// (model, voice, spoken text) in scripts/record/.cache/tts, so only new or
// changed lines are synthesised (and paid for).
//
//   node tts.js <out_dir>        → <out_dir>/narration.json (id, text, file, duration_s)
//
// Every new clip is checked: it is transcribed (default gpt-4o-mini-transcribe)
// and compared with the script; a clip that says something else (the TTS model
// sometimes switches language or adds words) is regenerated, up to 4 times.
//
// Needs OPENROUTER_API_KEY (read from the repo's .env if not set) and ffmpeg.
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { execFileSync } = require('child_process');

const ROOT = path.resolve(__dirname, '../..');
const OUT = process.argv[2] || path.join(__dirname, 'out');
const CACHE = path.join(__dirname, '.cache', 'tts');

function apiKey() {
  if (process.env.OPENROUTER_API_KEY) return process.env.OPENROUTER_API_KEY.trim();
  const env = fs.readFileSync(path.join(ROOT, '.env'), 'utf8');
  const m = env.match(/^OPENROUTER_API_KEY=(.*)$/m);
  if (!m) throw new Error('OPENROUTER_API_KEY not set');
  return m[1].trim();
}

async function synth(key, model, voice, text) {
  for (let attempt = 1; ; attempt++) {
    const r = await fetch('https://openrouter.ai/api/v1/audio/speech', {
      method: 'POST',
      headers: { Authorization: `Bearer ${key}`, 'Content-Type': 'application/json', 'X-Title': 'PPRD East TTX' },
      // Gemini TTS returns raw PCM (24 kHz, 16-bit, mono); mp3 is not offered.
      body: JSON.stringify({ model, voice, input: text, response_format: 'pcm' }),
    });
    if (r.ok) return Buffer.from(await r.arrayBuffer());
    const err = await r.text();
    if (attempt >= 3) throw new Error(`TTS ${r.status}: ${err.slice(0, 200)}`);
    await new Promise(res => setTimeout(res, 1500 * attempt));
  }
}

async function transcribe(key, model, wav) {
  const r = await fetch('https://openrouter.ai/api/v1/audio/transcriptions', {
    method: 'POST',
    headers: { Authorization: `Bearer ${key}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ model, input_audio: { data: fs.readFileSync(wav).toString('base64'), format: 'wav' } }),
  });
  if (!r.ok) throw new Error(`transcription ${r.status}: ${(await r.text()).slice(0, 200)}`);
  return (await r.json()).text || '';
}

const NUMBER_WORDS = new Set('zero one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty thirty forty fifty sixty seventy eighty ninety hundred thousand and'.split(' '));
const words = t => t.toLowerCase().replace(/[’']/g, '').split(/[^a-z0-9]+/).filter(w => w && !/^[0-9]+$/.test(w) && !NUMBER_WORDS.has(w));

/// Does the transcript say the script? Fractions of words missing / extra,
/// ignoring numbers (digits vs words) and punctuation.
function matches(expected, heard) {
  const e = words(expected), h = words(heard);
  const eSet = new Set(e), hSet = new Set(h);
  const missing = e.filter(w => !hSet.has(w)).length / Math.max(1, e.length);
  const extra = h.filter(w => !eSet.has(w)).length / Math.max(1, h.length);
  return { ok: missing <= 0.2 && extra <= 0.2, missing, extra };
}

(async () => {
  const spec = JSON.parse(fs.readFileSync(path.join(__dirname, 'narration.json'), 'utf8'));
  const { model, voice } = { ...spec.voice, voice: process.env.TTX_TTS_VOICE || spec.voice.voice };
  fs.mkdirSync(CACHE, { recursive: true });
  fs.mkdirSync(OUT, { recursive: true });
  const key = apiKey();
  let made = 0;
  let rejected = 0;
  const qaModel = (spec.qa && spec.qa.model) || 'openai/gpt-4o-mini-transcribe';
  const lines = spec.lines;
  const results = new Array(lines.length);
  // Small worker pool: a handful of requests at a time.
  let next = 0;
  async function worker() {
    while (next < lines.length) {
      const i = next++;
      const l = lines[i];
      const say = l.say || l.text;
      const h = crypto.createHash('sha1').update(`${model}|${voice}|${say}`).digest('hex').slice(0, 16);
      const wav = path.join(CACHE, `${h}.wav`);
      const qaFile = path.join(CACHE, `${h}.qa.json`);
      for (let attempt = 1; !fs.existsSync(qaFile); attempt++) {
        if (!fs.existsSync(wav)) {
          const pcm = await synth(key, model, voice, say);
          const raw = path.join(CACHE, `${h}.pcm`);
          fs.writeFileSync(raw, pcm);
          execFileSync('ffmpeg', ['-y', '-v', 'error', '-f', 's16le', '-ar', '24000', '-ac', '1', '-i', raw, wav]);
          fs.rmSync(raw);
          made++;
        }
        const heard = await transcribe(key, qaModel, wav);
        const m = matches(say, heard);
        if (m.ok || attempt >= 4) {
          fs.writeFileSync(qaFile, JSON.stringify({ heard, ...m, attempts: attempt }, null, 1));
          if (!m.ok) console.warn(`WARNING ${l.id}: still differs after ${attempt} attempts: "${heard}"`);
        } else {
          console.log(`retry ${l.id} (attempt ${attempt}): heard "${heard.slice(0, 120)}"`);
          fs.rmSync(wav);
          rejected++;
        }
      }
      const dur = parseFloat(execFileSync('ffprobe', ['-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', wav]).toString());
      results[i] = { id: l.id, who: l.who, text: l.text, shows: l.shows, file: wav, duration_s: dur };
    }
  }
  await Promise.all(Array.from({ length: 4 }, worker));
  fs.writeFileSync(path.join(OUT, 'narration.json'), JSON.stringify({ model, voice, lines: results }, null, 1));
  const total = results.reduce((a, r) => a + r.duration_s, 0);
  console.log(`narration: ${results.length} lines (${made} synthesised, ${rejected} rejected by the transcription check), ${total.toFixed(0)} s of speech, voice ${voice}`);
})().catch(e => { console.error(e); process.exit(1); });

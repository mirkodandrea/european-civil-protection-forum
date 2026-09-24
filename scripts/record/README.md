# Recording the narrated demo

`record.sh` produces `docs/media/ttx_demo.mp4`: a scripted two-turn session in which every role is played in the phone-sized all-roles page (`play.html`, left) next to the 3D simulator with its cinematic camera (right), narrated by Google Gemini TTS through OpenRouter, with subtitles.

```bash
cargo build --release -p app
./scripts/record/record.sh              # → docs/media/ttx_demo.mp4, .srt, .vtt, _captions.json, _log.jsonl
python3 scripts/record/readme_demo.py   # refresh the timeline and outcome tables in docs/demo.md
```

Options (environment):

| Variable | Effect |
|---|---|
| `TTX_REC_FAST=1` | Hardware H.264 for the final pass (about 3× faster, bigger file) |
| `TTX_REC_FRESH=1` | Ask the LLM again instead of replaying `llm_cassette.jsonl` (and re-record it) |
| `TTX_TTS_VOICE` | Gemini voice (default `Charon`, set in `narration.json`) |
| `TTX_REC_PORT` | Port of the recording `ttx` (default 8090; a game on :8080 is untouched) |

Requirements: `.env` with `OPENROUTER_API_KEY` (only needed for new narration lines or fresh LLM answers), Google Chrome, Node, ffmpeg. A second simulator window opens; keep it on screen while recording.

## Pipeline

| Step | Tool | What it does |
|---|---|---|
| 1. Narration | `tts.js` | Reads `narration.json` (the story of the incident) and synthesises one WAV per line with `POST /api/v1/audio/speech` (`google/gemini-3.1-flash-tts-preview`, raw 24 kHz PCM → WAV). **Every new clip is transcribed** (`openai/gpt-4o-mini-transcribe` via `/api/v1/audio/transcriptions`) and compared with its script; clips that drift (dropped sentences, a switch to another language, which the model did once when a line said "in Italian") are regenerated, up to 4 times. Cached in `.cache/tts/` by (model, voice, text), so only new or changed lines are paid for |
| 2. Subtitles | `render_subs.js` | Renders one transparent PNG per line plus the title bar with Chrome (this ffmpeg build has no text rendering) |
| 3. Simulator | `ttx` with `TTX_CINEMATIC=1` | Cinematic camera. While `runs/rec/ON` exists, every 2nd rendered frame (~25 fps) is **piped straight into ffmpeg** and encoded while recording (hardware H.264, wall-clock timestamps) into `runs/rec/sim.mkv` (`viz::record`). The LLM runs through `TTX_LLM_CASSETTE` = `llm_cassette.jsonl`: answers are replayed, misses recorded |
| 4. Session | `session.js` | Playwright drives Chrome at 480×1040 through `play.html` and records the page (encoded while recording). Each `caption(id)` marks where a narration line starts and **waits until the previous line has been spoken**, so the voice paces the session. Waits on API state, never on fixed LLM delays |
| 5. Compose | `compose.py` | One ffmpeg pass: phone + simulator (synced by their start times) + title + burned-in subtitles under the simulator + narration mixed on the marks (stereo AAC) + a soft subtitle track; writes `.srt` and `.vtt` |

## Speed

| Run | Time for a ~4 min video |
|---|---|
| Before (PNG frames, live LLM, no narration) | ~6.5 min |
| First narrated run (live LLM, TTS synthesis) | ~6 min |
| Re-recording (cassette + TTS cache), x264 | ~4 min 50 s |
| Re-recording with `TTX_REC_FAST=1` | ~4 min 25 s |

Recording is real time by nature (the browser and the window are captured live), so the floor is the video's own length. Everything around it is gone: no LLM waits, no TTS calls, no frame files, and both streams are already encoded when the session ends.

## Files

- `narration.json`: the script (`text` = subtitle, `say` = what the voice reads when it differs, `who` / `shows` for the timeline in docs/demo.md)
- `llm_cassette.jsonl`: recorded model answers (committed, so anyone can re-record the same video without calling the model)
- `session.js`, `tts.js`, `render_subs.js`, `compose.py`, `record.sh`, `readme_demo.py`

A single screenshot of the simulator window: press `P`, or write a target path into `runs/shot.req`.

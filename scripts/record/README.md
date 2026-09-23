# Recording a demo session

`record.sh` produces `docs/media/ttx_demo.mp4` (plus `ttx_demo_captions.json` and the run's event log `ttx_demo_log.jsonl`): a scripted two-turn session in which every role is played in the phone-sized all-roles page (`play.html`, left) next to the 3D simulator (right), with captions explaining each step.

```bash
cargo build --release -p app
./scripts/record/record.sh                 # → docs/media/ttx_demo.mp4 (+ _captions.json)
```

Requirements: `.env` with `OPENROUTER_API_KEY` (the session uses the real LLM), Google Chrome, Node, ffmpeg. The script runs its own `ttx` on port 8090 (`TTX_REC_PORT`) in a temporary directory, so a game on :8080 is untouched. A second simulator window opens; keep it on screen while recording.

## How it works

| Piece | What it does |
|---|---|
| `session.js` | Playwright drives Chrome at 480×1040 (1:1 pixels) through `play.html`: switches role tabs, types replies (Italian, English, Armenian), clicks approvals, and waits for the LLM through the API. It records the page to WebM and writes `video_start_ms.txt` plus caption timestamps (`marks.json`) |
| Turn speed | The session sets the Director's speed to 120×, so each turn plays out live on the big screen (30 min in 15 s), and waits for the phase to return to `orders` |
| Simulator instrumentation | While `runs/rec/ON` exists, the Bevy app saves a frame every 100 ms as `runs/rec/<unix-ms>.png` (`viz::record`). `TTX_ORBIT=1` starts the camera in slow auto-orbit; `TTX_CAM` (default for recordings: `TTX_REC_CAM`, `2800,2450,1700,0.7,0.52`) sets the view. `session.js` creates and removes the flag file |
| `compose.py` | Lays the timestamped simulator frames on the browser video's timeline (ffmpeg concat demuxer with per-frame durations, so the halves stay in sync) and composes 1920×1080 H.264 |

The LLM answers differently on each run, so the session waits on API state rather than fixed delays (e.g. "no proposal still parsing"), and approvals target whatever the model produced. Tune the pacing with the `sleep`/hold values in `session.js`; tune the size/quality with `TTX_REC_CRF` (default 33 ≈ 22 MB for 4.5 min).

Separately, a single screenshot of the simulator window can be taken at any time by pressing `P` or by writing a target path into `runs/shot.req`.

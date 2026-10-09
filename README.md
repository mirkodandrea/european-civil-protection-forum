# PPRD East 4 — AI-supported Table-Top Exercise (working draft)

A turn-based wildfire TTX for the Civil Protection Forum 2026:

- a **simulator** (Rust + Bevy): PROPAGATOR-core fire spread as a 32-run Monte Carlo ensemble (the fire is the set of cells reached by at least half of the runs), buildings with occupants, fire lines and water drops, a 3D big-screen view that shows each turn being simulated live, second by second;
- an **Exercise Director (Master)** web console: injects (MSEL), turn control, approval of every player action, god-mode map tools, AI prompts, commands by code, rollback;
- **player** web pages (tablets): role-specific inbox in the player's own language, free-text replies;
- an **LLM** (OpenRouter, `deepseek/deepseek-v4.1-flash`) that translates injects, turns replies into candidate commands, drafts clarification questions when orders are vague, and translates recorded feedback and situation updates.

> Humans decide. Rules constrain. The model simulates. AI mediates. The Master approves.

Background: [`PPRD_East_4_Agentic_TTX_Use_Case_for_Mirko.md`](PPRD_East_4_Agentic_TTX_Use_Case_for_Mirko.md) (methodology) and [`TTX_Software_Spec.md`](TTX_Software_Spec.md) (target spec, with an as-built section).

## Demo

[![Demo: one phone plays every role next to the 3D simulator, narrated (4 min)](docs/media/ttx_demo_poster.jpg)](docs/media/ttx_demo.mp4)

**[▶ Watch the demo: docs/media/ttx_demo.mp4](docs/media/ttx_demo.mp4)** (4 min 18 s, 1920×1080, 27 MB, **narrated** by Google Gemini TTS, voice "Charon": the story of the incident as it unfolds, with burned-in subtitles plus a subtitle track; separate [`.srt`](docs/media/ttx_demo.srt) and [`.vtt`](docs/media/ttx_demo.vtt))

What happens in the video, and how to play or re-record it: [docs/demo.md](docs/demo.md).

## Quick start

```bash
# .env in the repo root
OPENROUTER_API_KEY=sk-or-...
# OPENROUTER_MODEL=deepseek/deepseek-v4.1-flash   (default)

cargo run --release -p app            # simulator window + web server on :8080
cargo run --release -p app -- --headless   # web server only (no window)
```

Then open:

| Who | URL |
|---|---|
| **All roles in one window** (Director + every player as tabs; good on a phone) | Startup Master link with access key |
| Exercise Director | `http://<host>:8080/master.html` |
| Players (tablets) | Individual role link from the Master console |
| Landing page | `http://<host>:8080/` |

The server prints a fresh Master access link at startup. Open it, then distribute individual player links from the console; each key authorizes only its own role. The simulator window shows the machine's LAN address. Without an OpenRouter API key the exercise still runs in **manual mode**: injects are sent in English and the Master types the commands for each proposal.

### Playing from a phone

`play.html` puts the Director and every player in tabs of one page (a dot marks new messages or open proposals), laid out for phones. To reach it from outside the LAN, use a [Microsoft dev tunnel](https://aka.ms/devtunnels/docs):

```bash
brew install --cask devtunnel
devtunnel user login -g -d        # GitHub device-code login
devtunnel host -p 8080            # private: only your account can open the URL
# open the tunnel URL with the startup Master access key, or use an individual player link (GitHub login once)
```

Keep the tunnel private (the default). With `--allow-anonymous`, anyone with the URL could run the Director console and spend your OpenRouter credit.

Other options: `--scenario FILE`, `--seed N`, `--port N`, `--web DIR`, `--realizations N` (Monte Carlo runs, default 32), `--cutoff P` (fraction of runs that must reach a cell for it to count as burned, default 0.5; 0.25 gives a pessimistic, planning fire). The synthetic scenario is (re)generated with `cargo run -p scenario --bin gen-scenario`.

## Playing a turn

1. **Director**: *Start exercise*, then send MSEL injects (*Injects* tab) or custom ones. Each player receives them translated.
2. **Players** choose and confirm their language, cite a procedure/plan or explain their operational judgment, optionally link supporting evidence, and reply in their own words ("evacuate the school to the sports centre, close the northern road"). The AI drafts commands and communications, checked against the role's authority and resources. Draft messages and clarification questions wait for Director review.
3. **Director** approves, edits (JSON) or rejects each proposal (*Proposals* tab). Players get feedback in their language. The Director can also act directly: map tools (ignite, fire line, drop, extinguish zone, evacuate/warn zone, building status), *AI prompt* ("the fuel station explodes; tell the mayor"), or *Commands* (JSON).
4. **Resolve turn ▶** (Δt, default 30 min): approved actions are executed, then the turn is **simulated live, one simulated second at a time**. The 3D view redraws continuously, the browsers refresh up to 4 times a second, and when the turn ends every player receives a situation update. **speed** (next to Δt) is simulated seconds per real second: 0 = maximum speed (a 30-minute turn in about 0.45 s), 60× = one simulated minute per second (30 s per turn), 120× = 15 s per turn.
5. Repeat. Use *Rollback* to return to the start of an earlier turn and *End* to finish. *Debrief JSON* gives the full timeline.

Places can be named ("Pineta Primary School", "Pine Hill"), given as **zone codes** from the map grid (`F4`), as zone edges for fire lines (`F4:N`), or as coordinates.

`scripts/demo_session.sh` plays a full two-turn session through the API; `scripts/record/record.sh` plays one in the browser and records the demo video (Italian, English and Armenian players; an AI prompt; a clarification; reinforcements).

## Screenshots

| | |
|---|---|
| ![Master — proposals](docs/screenshots/master_proposals.png) Director: proposals with translation, AI summary and feasibility checks | ![Player — Mayor, Italian](docs/screenshots/player_mayor_italian.png) Mayor playing in Italian |
| ![Player — regional, Armenian](docs/screenshots/player_regional_armenian.png) Regional Civil Protection playing in Armenian | ![Player — clarification](docs/screenshots/player_fire_commander_clarification.png) A vague order gets a clarification question |
| ![Master — commands by code](docs/screenshots/master_commands_by_code.png) Commands by code | ![Turn 2 simulated live](docs/screenshots/sim_turn2_live.png) Turn 2 simulated live: the lines hold above Pineta, the second fire runs through the farmland |
| ![Flames close-up](docs/screenshots/sim_flames_closeup.png) Flames sized by ensemble intensity and probability, tilted by the wind; uncertainty glow ahead, charcoal scar behind | |

## Layout

```
crates/propagator-core  PROPAGATOR fire-spread core (vendored, std-only)
crates/scenario         scenario package + synthetic "Valverde valley" generator
crates/fire             fire model wrapper: ignition, fire lines, drops, extinguish, snapshots
crates/sim              world state, Command catalogue, validation, buildings, resources, digests
crates/llm              OpenRouter gateway + prompt workflows (mockable)
crates/exercise         turn engine: injects, inboxes, proposals, approval, rollback, event log
crates/server           axum HTTP/WebSocket API + LLM orchestration
crates/viz              Bevy big-screen renderer (3D terrain, vegetation, fire, live turn view, HUD)
crates/app              the `ttx` binary
web/                    Master, player, all-roles (play.html) and landing pages (plain HTML/JS, no build step)
scripts/record          scripted + recorded browser session and video composition (Playwright, ffmpeg)
assets/models           Blender vegetation meshes (from propagator_abm)
data/scenarios          generated scenario JSON
docs/                   architecture, API, commands, simulation, LLM, scenario docs; media/ demo video
runs/                   per-run event logs (JSONL) and screenshots (git-ignored)
```

## Tests

```bash
cargo test --release --workspace                 # 104 tests, headless, mock LLM
TTX_LIVE_LLM=1 cargo test -p llm --test live -- --nocapture   # real OpenRouter call
```

The server tests play the whole Master ↔ player ↔ LLM loop over HTTP with a mock model, and also without any model.

## Docs

- [docs/architecture.md](docs/architecture.md): components, threads, data flow, turn loop
- [docs/api.md](docs/api.md): HTTP/WebSocket API (drive everything by code)
- [docs/commands.md](docs/commands.md): the command catalogue and location syntax
- [docs/simulation.md](docs/simulation.md): fire, buildings, evacuation, resources, calibration
- [docs/llm.md](docs/llm.md): LLM workflows, prompts, guardrails, fallback
- [docs/scenario.md](docs/scenario.md): the synthetic scenario, roles, MSEL, how to make another
- [docs/demo.md](docs/demo.md): the demo video described: incident, roles, timeline, players' words, outcome
- [docs/todo.md](docs/todo.md): what is not built yet; [docs/specs/](docs/specs/README.md): draft specs for v0.3
- [docs/screenshots](docs/screenshots): captures from a live session
- [docs/media/ttx_demo.mp4](docs/media/ttx_demo.mp4): the demo video; [scripts/record/README.md](scripts/record/README.md) explains how it is recorded

Player maps show reported locations and perimeter observations delivered to that
role. The Master can attach map data to injects or publish a reviewed perimeter;
undisclosed simulation changes remain hidden. See [role access and map API](docs/api.md#role-access-and-map-information).
Scripted demos require `TTX_MASTER_KEY` from the startup link.

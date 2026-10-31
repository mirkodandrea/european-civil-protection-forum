# PPRD East 4 — AI-supported Table-Top Exercise (working draft)

**WIP — work in progress.** Il progetto è in fase di definizione: funzionalità,
requisiti e direzione possono ancora cambiare.

The browser frontend is written in **Rust with Yew**, compiled to WebAssembly. **Rocket** serves its generated assets, JSON APIs and WebSockets. Application state, forms, chats, maps and live updates run in Rust; JavaScript is limited to the generated WebAssembly loader.

The current capabilities are summarized in [docs/stato-attuale.md](docs/stato-attuale.md). EXCON and players use chats, reviewed action proposals and separate map views; the previous director console remains at `advanced.html`.

A turn-based wildfire TTX for the Civil Protection Forum 2026:

- a **simulator** (Rust + Bevy): PROPAGATOR-core fire spread as a 32-run Monte Carlo ensemble (the fire is the set of cells reached by at least half of the runs), buildings with occupants, approved deployed teams travelling on the simulation clock, fire lines and water drops, a 3D big-screen view that shows each turn being simulated live, second by second;
- an **EXCON** web console: injects (MSEL), turn control, actions suggested from the players' replies (executed, edited or ignored by the EXCON; players never act), god-mode map tools, AI prompts, commands by code, rollback;
- **player** web pages (tablets): role-specific inbox in the player's own language, free-text replies;
- an **LLM** (OpenRouter, `deepseek/deepseek-v4.1-flash-1017`) that translates injects, reads each reply thread into catalogue actions for the EXCON, drafts clarification questions (always reviewed by the EXCON before sending) when orders are vague, and translates recorded feedback and situation updates.

> Humans decide. Rules constrain. The model simulates. AI mediates. The EXCON approves.

Deployed units travel from configurable training staging bases. Fire-line, water-drop,
bus evacuation and road-closure task effects begin on actual arrival; the 3D view
shows travel, arrival and task status from simulation records. Pending proposals
never create team markers.

Background: [`PPRD_East_4_Agentic_TTX_Use_Case_for_Mirko.md`](PPRD_East_4_Agentic_TTX_Use_Case_for_Mirko.md) records the initial use case. The current scope is in [docs/stato-attuale.md](docs/stato-attuale.md); implementation details are in code.

## Standalone WebAssembly

Le versioni pubblicate sono disponibili su [GitHub Pages](https://mirkodandrea.github.io/european-civil-protection-forum/): [main](https://mirkodandrea.github.io/european-civil-protection-forum/main/) e [dev](https://mirkodandrea.github.io/european-civil-protection-forum/dev/). Build, differenze e verifica: [docs/standalone-pages.md](docs/standalone-pages.md).

The landing page has one card per branch: main is the version being playtested,
dev the next one. Each card's title, description, highlights and mode links
(`?bench=1`, `?simbench=1`, `?editor=1`) come from that branch's
[`release.json`](release.json); update it when the branch's content changes
(fields and the dev example: [docs/standalone-pages.md](docs/standalone-pages.md#pagina-iniziale-e-releasejson)).

`./scripts/build_standalone.sh` builds **`web/standalone/ttx.html`**, one
self-contained HTML app with the real simulation, EXCON, all player views and
the Bevy 3D renderer in the simulation panel. It runs directly in a browser
with GPU support, offline and without a backend; OpenRouter keys and models can
be configured in the app for direct AI requests. Standalone role tabs need no
access keys; each opening has its own in-memory exercise. The server-hosted
version continues to use EXCON and player access links.
Build and serve it locally with `./scripts/run_pages_local.sh` as described below.

To build and serve the standalone app locally, run `./scripts/run_pages_local.sh`
and open `http://127.0.0.1:8000/`. It reads `OPENROUTER_API_KEY` from `.env`,
uses `openai/gpt-oss-120b` for both tasks, and accepts a port argument or
`TTX_PAGES_PORT`. This local artifact embeds the key; do not publish it.

## Quick start

```bash
# One-time frontend tools (Trunk and wasm-bindgen are already available on the development machine)
rustup target add wasm32-unknown-unknown
cargo install trunk --locked

# Development: frontend rebuild + browser live reload
./scripts/dev_frontend.sh

# .env in the repo root
OPENROUTER_API_KEY=sk-or-...
# OPENROUTER_MODEL=deepseek/deepseek-v4.1-flash-1017   (default)
# OPENROUTER_PROVIDERS=together   (default, only these hosts, no fallback; "auto" = OpenRouter chooses)

cargo run --release -p app            # builds the browser UI if needed, then starts window + web server on :8080
cargo run --release -p app -- --headless   # native server only (no window)
```

During frontend development, `./scripts/dev_frontend.sh` runs the full
standalone WASM app through Trunk on `http://127.0.0.1:8080`, with automatic
rebuild and browser refresh. It reads `OPENROUTER_API_KEY` from `.env` like
`run_pages_local.sh`; the key is compiled into the browser app, so keep this
local build private. Changes in `crates/frontend/` are picked up automatically.

`./scripts/dev_bench.sh` serves the dev-only workflow test bench (EXCON and
player screens side by side on a simulated server, no AI) at
`http://127.0.0.1:8080/?bench=1`; see [docs/banco-di-prova.md](docs/banco-di-prova.md).
The standalone build also serves a simulator bench (3D left, EXCON Controllo
right, clock play/pause, tools applied at any time) at
`http://127.0.0.1:8080/?simbench=1`; see [docs/banco-simulatore.md](docs/banco-simulatore.md).

Then open:

| Who | URL |
|---|---|
| **Conversational EXCON interface** | `http://<host>:8080/play.html` |
| EXCON | `http://<host>:8080/master.html` |
| Players (tablets) | Individual role link from the EXCON console |
| Landing page | `http://<host>:8080/` |

There are no access keys: EXCON is `play.html`/`master.html`, players open `player.html?role=<id>` (links are listed in the console). Anyone who can reach the port can act as any role, so run it only on a trusted network. The simulator window shows the machine's LAN address. Without an OpenRouter API key the exercise still runs in **manual mode**: injects are sent in English and the EXCON types the commands for each proposal.

### Playing from a phone

`play.html` opens the EXCON conversational console. Players use their individual role links, with a responsive chat and tactical map for phones and tablets. To reach it from outside the LAN, use a [Microsoft dev tunnel](https://aka.ms/devtunnels/docs):

```bash
brew install --cask devtunnel
devtunnel user login -g -d        # GitHub device-code login
devtunnel host -p 8080            # private: only your account can open the URL
# open the tunnel URL (GitHub login once)
```

Keep the tunnel private (the default). With `--allow-anonymous`, anyone with the URL could run the EXCON console and spend your OpenRouter credit.

The `app` crate builds the browser UI into `web/dist/` when its frontend inputs change, before starting the server. A standalone `./scripts/build_frontend.sh` is still available; the server defaults to `--web web/dist`. No Node build step or CDN is required.

Other options: `--scenario FILE`, `--seed N`, `--port N`, `--web DIR`, `--realizations N` (Monte Carlo runs, default 32), `--cutoff P` (fraction of runs that must reach a cell for it to count as burned, default 0.5; 0.25 gives a pessimistic, planning fire). The default landscape is the Factory-generated Valverde valley. Regenerate it with `python3 scripts/scenario_factory.py landscape` (requires NumPy). The historical calibrated scenario remains available with `--scenario data/scenarios/synthetic_valley.json`; `cargo run -p scenario --bin gen-scenario` regenerates that historical template.

## Model / 3D viewer split

The exercise and its 3D view are independent. `crates/view` defines the
serde contract (`ViewState`, `Deployment` tracks, grid arrays packed in
base64); `server::view` produces it from the exercise; `viz` (Bevy) draws it
and has no dependency on `sim`, `fire` or `exercise`. Three transports share
that contract:

- in-process (`ttx` window, and the standalone WASM app): `ExerciseSource`;
- HTTP: `GET /api/view/scene` (once; `scene.z` is its zlib form, 12.6 MB → 3.1 MB) and `GET /api/view/state?generation=&cue=`
  (`null` if nothing is new). EXCON identity only, because the view shows the
  ground truth;
- compact HTTP: `GET /api/view/frame?generation=&cue=` answers `204` when up to
  date, otherwise a zlib-deflated JSON frame: the grid arrays and road flags as
  sparse patches against the viewer's generation when the server still holds
  it, or the whole state. Measured on the Valverde scenario: 1.0 MB JSON state →
  46 KB full frame → about 2 KB per simulated minute;
- remote native screen: `ttx --headless` on the server, then
  `ttx-viewer --server http://host:8080`.

An HTML/WebGL viewer only has to implement the same two calls.

## Operational model from propagator_abm

`cargo run --release -p app -- --operational-model` enables road routing,
progressive fire-line construction, engines with finite tanks and refills,
finite aircraft drops, evacuation queues and intensity-based exposure.
Existing scenarios keep their calibrated rules unless
this mode is enabled. Scenario configuration and command behavior are defined in
the scenario and simulation crates.

The updated operational model adds segment-local fire closures, terrain-aware
travel and queue delays, and local execution policies inside an approved mission.
The renderer defaults to a stable, readable projector profile. The graphics level
(`low`, `projector`, `high`) is picked in the big-screen HUD or cycled with `Q`, applies
live (shadows, cascades, MSAA/SMAA, bloom, fire lights; terrain mesh and plant density
rebuild the scene) and is remembered in `~/.ttx/graphics.txt`. `TTX_QUALITY=` wins over
the remembered level and `TTX_GFX=-bloom,msaa=1,shadow=1500,veg=0.3,stride=2` overrides
single knobs. EXCON can compute private forecast drafts and publish reviewed
products to selected roles. The implementation is in the simulation, exercise
and visualization crates; snapshot tooling is under `scripts/`.

The native big screen now uses the latest source PBR pipeline: separate terrain,
foliage and structure materials, GPU vegetation burning, sky reflections and
articulated engine/tanker/firefighter models. Houses have detailed roofs and
facades, and imported open spaces retain their original visual category. The
browser retains its 2D operational map. `sh scripts/qa/pbr.sh` captures the
native renderer invisibly and checks for shader errors before visual review.
The game flame ribbons, smoke/ember shaders and ground glow are integrated too.
Its continuous fire-field reconstruction now smooths the perimeter and individual
plant ignition; `V` compares with the original cells. The HUD shows the physical
model resolution; this visual upscaling does not change simulation results.
`python3 scripts/snapshot.py --out runs/new --spec 'name=fire,t=30,at=fire,ui=0,marks=0'`
prepares a batch of real exercise states, saves PNGs and an HTML review report;
`--bench 3` adds CPU/render/GPU measurements. See the graphics guide for keys.

## Playing a turn

1. **EXCON**: wait until every participant has confirmed a language (the console lists who is missing; *Avvia comunque* overrides), then start the exercise. Players see a waiting screen until then. Every new or restarted exercise asks each participant again (the scenario's language is proposed); in the standalone app each role tab asks.
   Open a player conversation and choose **Scrivi come → Inject** in the chat composer. Write a custom message or load an MSEL template; select the conversation recipient or explicitly use the template audience. Each player receives a translated chat message and replies in the same conversation.
2. **Players** write in the conversation, in their language. With **Prepara decisione con l’AI**, interpretation happens privately: the assistant asks the player specific questions when details are missing. The player reviews the final message/actions and explicitly confirms delivery. EXCON receives only that confirmed message and proposal. Ordinary information can be sent directly by disabling decision preparation.
3. **EXCON** approves, edits (JSON) or rejects each proposal (*Risposte* tab), or approves only some of its actions (each left-out action can carry a reason for the player); a reply with nothing to execute is acknowledged. Lists show the verdict (approved / partially approved / rejected / acknowledged / clarification requested). Players get feedback in their language, action by action. The EXCON can also act directly: map tools (ignite, fire line, drop, extinguish zone or drawn area, evacuate/warn zone, building status), *AI prompt* ("the fuel station explodes; tell the mayor"), or *Commands* (JSON).
4. **Close turn ▶▶** in the Timeline's turn panel (runs to the end of the turn, default 30 min; the panel also offers intermediate advances): approved actions are executed, then the turn is **simulated live, one simulated second at a time**. The 3D view redraws continuously, the browsers refresh up to 4 times a second, and when the turn ends every player receives a situation update. **speed** is simulated seconds per real second: 0 = maximum speed (a 30-minute turn in about 0.45 s), 60× = one simulated minute per second (30 s per turn), 120× = 15 s per turn.
5. Repeat. Use *Rollback* to return to the start of an earlier turn and *End* to finish. *Debrief JSON* gives the full timeline.

Places can be named ("Pineta Primary School", "Pine Hill"), given as **zone codes** from the map grid (`F4`), as zone edges for fire lines (`F4:N`), or as coordinates.

`scripts/demo_session.sh` plays a full two-turn session through the API. The narrated demo-video pipeline has been removed for now; `.claude/skills/demo-video-recording` records how to rebuild it.

## Layout

```
crates/propagator-core  PROPAGATOR fire-spread core (vendored, std-only)
crates/scenario         scenario package + synthetic "Valverde valley" generator
crates/fire             fire model wrapper: ignition, fire lines, drops, extinguish, snapshots
crates/sim              world state, Command catalogue, validation, buildings, resources, digests
crates/llm              OpenRouter gateway + prompt workflows (mockable)
crates/exercise         turn engine: injects, inboxes, proposals, approval, rollback, event log
crates/server           Rocket HTTP/WebSocket API + LLM orchestration
crates/frontend         Yew Rust/WebAssembly UI (chat, canvas maps, EXCON, evaluator)
crates/viz              Bevy big-screen renderer (3D terrain, vegetation, fire, live turn view, HUD)
crates/app              the `ttx` binary
web/dist/               Generated HTML entry points, CSS, WebAssembly and loader (Trunk build)
scripts/qa              browser QA scripts (Playwright; npm ci --prefix scripts/qa)
assets/models           Blender vegetation, town and articulated PBR unit bakes
data/scenarios          generated scenario JSON
docs/                   high-level capabilities and open topics
runs/                   per-run event logs (JSONL) and screenshots (git-ignored)
```

## Tests

```bash
cargo test --release --workspace                 # 199 tests incl. doctest; headless, mock LLM
TTX_LIVE_LLM=1 cargo test -p llm --test live -- --nocapture   # real OpenRouter call
```

The server tests play the whole EXCON ↔ player ↔ LLM loop over HTTP with a mock model, and also without any model.

## Desktop builds and releases

`.github/workflows/desktop.yml` builds `ttx` for Windows, macOS and Linux (zip with
the scenarios, the web console, a launcher script and `env.example` for the
OpenRouter key) and smoke-tests each bundle. Every push to `main` refreshes the
pre-release `download`; a version is released from `main` with

```bash
./scripts/release.sh 0.2.0   # sets the workspace version, commits, tags v0.2.0, pushes
```

and the workflow publishes the `v0.2.0` release with the three zips.

## Docs

- [docs/stato-attuale.md](docs/stato-attuale.md): specifica di alto livello delle capacità presenti
- [docs/todo.md](docs/todo.md): pochi temi ancora da definire

The implementation and detailed behavior can be read from the source code.

Player maps show reported locations and perimeter observations delivered to that
role. The EXCON can attach map data to injects or publish a reviewed perimeter;
undisclosed simulation changes remain hidden. Role-specific access is enforced by the server API.
Scripted demos require `TTX_MASTER_KEY` from the startup link. The console also
provides three evaluator links: each opens a read-only evidence dashboard with
chains, objectives, supporting documents, filters, and full debrief export.

EXCON can run a configurable clock within an open turn and schedule MSEL injects
by simulated time. Training phases (early warning, early action, escalation,
full response) preserve accumulated state and are independent of decision turns.
Clock progression runs physics and arrivals; approved orders execute only when
EXCON explicitly executes them or resolves the turn. The server and exercise crates implement clock and phase behavior.

Resource deployment respects physical inventory plus reviewed exercise-wide and
per-training-phase caps. Active and approved units count together; requests and
returns stay distinct. AI-recommended limits remain drafts until EXCON approval.

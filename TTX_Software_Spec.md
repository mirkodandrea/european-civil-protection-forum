# PPRD East 4 – Agentic TTX: Software Components & Specs

Concise technical spec for the Civil Protection Forum 2026 prototype.
Methodology and rationale: `PPRD_East_4_Agentic_TTX_Use_Case_for_Mirko.md`.
Reference implementation for sim/ABM/rendering: `~/dev/experiments/propagator_abm`.

---

## 1. Decisions

| Topic | Decision |
|---|---|
| Simulator | New Rust + Bevy application. Includes the wildfire model, the ABM, the rich visualisation and the web server |
| Fire model | PROPAGATOR-like cellular model running **in-process**. No external PROPAGATOR service. `propagator-core` (std-only Rust) can be vendored as in the reference repo |
| ABM | Households, people, response units, traffic on the real road network. Same design as the reference `abm` crate |
| Game model | **Turn-based.** Simulation time advances only when the Master resolves a turn or skips time |
| Master | Omnipotent within what the simulator supports. Acts through GUI controls **or** natural-language prompts that the LLM turns into commands |
| Players | Receive injects translated and adapted by the LLM. Reply in free text. The LLM turns replies into actions, limited to that role's resources and permitted actions. **Every player action needs Master approval** |
| Interfaces | Master and players use web browsers (tablets/laptops). The big screen shows the Bevy window |
| AI failure | The exercise continues: Master enters commands through the GUI and sends injects untranslated or pre-translated |

---

## 2. Architecture

One process on the stand machine. The simulation lives on the Bevy main thread; the web server and LLM calls run on a background async runtime; the two sides talk over channels. This is the reference repo's `api.rs` pattern, scaled up.

```mermaid
flowchart LR
    subgraph Browsers
        M[Master UI]
        P[Player UI x N]
        O[Observer / Evaluator UI]
    end

    subgraph Process["ttx binary (single process)"]
        subgraph Async["tokio runtime"]
            WS[Web server<br/>HTTP + WebSocket]
            EX[Exercise engine<br/>turns · injects · proposals]
            LLM[LLM gateway]
        end
        CH{{command / query channels}}
        subgraph Bevy["Bevy main thread"]
            SIM[Sim core<br/>fire + ABM + world state]
            VIZ[Visualisation<br/>3D / 2D big screen]
        end
        LOG[(Event log<br/>SQLite)]
        SNAP[(Checkpoints)]
    end

    M & P & O <--> WS
    WS <--> EX
    EX <--> LLM
    LLM <--> PROV[(OpenRouter / Anthropic /<br/>Ollama local)]
    EX <--> CH <--> SIM
    SIM --> VIZ
    EX --> LOG
    SIM --> SNAP
```

**Rule:** only the sim core mutates world state, and only by executing `Command`s. The LLM never touches the sim. It only produces candidate `Command`s, and those are validated by the sim and then approved by the Master.

---

## 3. Components (cargo workspace)

Dependency direction is strict, as in the reference repo: model crates are Bevy-free and have fast headless tests. Only `viz` and `app` depend on Bevy.

| Crate | Responsibility | Reuse from `propagator_abm` |
|---|---|---|
| `scenario` | Loads the scenario package (§7): terrain, fuel, OSM, population, facilities, resources, roles, gazetteer, MSEL. World coordinate frame | `scenario` + bake scripts |
| `propagator-core` | Fire-spread kernel, checkpoint | vendored as-is |
| `fire` | Stepping, weather, ignitions, interventions (fuel cut, moisture), exposure/threat, direct perimeter editing | `fire` |
| `abm` | Population (perception → decision → preparation → movement), traffic, havens/shelters, units and tasks, closures, external assets arriving with delay | `abm`, `behavior` (optional) |
| `sim` | The **world state** combining fire + ABM + infrastructure + resources. `Command` catalogue with `validate()` / `apply()`, `advance(dt)`, snapshot/restore, role-filtered **state digests** for the LLM and UIs | new (generalises `game::sim`) |
| `exercise` | Turn state machine, roles and capability manifests, MSEL/injects, inbox, proposals + approval queue, per-role visibility. No Bevy, no HTTP | new |
| `llm` | Provider abstraction (cloud + Ollama), prompt templates, tool/JSON schemas generated from the `Command` catalogue, retries/timeouts, circuit breaker | `chat` provider/config |
| `telemetry` | Append-only event log (SQLite): every inject, message, proposal, decision, command, turn, checkpoint id | `telemetry` |
| `frontend` | Rust/Yew WebAssembly browser UI: chat, maps, review, EXCON and evaluator | replaces hand-written web JavaScript |
| `server` | Rocket HTTP + WebSocket, role-token auth, serves the web UIs, pushes updates | new (replaces `tiny_http` API) |
| `viz` | Bevy rendering: terrain, fire, smoke, buildings, agents, units, overlays. Public-display HUD (turn, clock, latest injects, decision → consequence ticker). Replay of a resolved turn at accelerated speed | `game` renderers |
| `app` | Binary: wires Bevy + tokio + channels. Modes `--headless` (tests/CI, no window) and `--display` | new |
| `web/` | Master, player and observer SPAs (TypeScript). 2D map fed by state tiles/GeoJSON | new |

---

## 4. Command catalogue

All state changes are `Command`s: a single typed Rust enum. The same types generate (a) the LLM tool schemas, (b) the Master GUI forms and (c) the event-log records. **The list will grow** from the methodology docs and future sessions. Initial set:

**Master-only (god mode)**
- Time: `Advance{dt}`, `SkipTo{t}` (fast-forward or state override), `Pause/Resume`, `SaveCheckpoint`, `LoadCheckpoint{id}`, `Reset`
- Fire: `Ignite{area}`, `SetWeather{wind_dir,wind_speed,moisture}`, `SetBurnt{polygon}`, `Extinguish{polygon}`, `SetFuel{polygon,class}`
- Buildings/facilities: `SetBuildingStatus{id|area, occupied|evacuated|damaged|destroyed}`, `SetOccupancy{facility,n}`
- Infrastructure: `SetInfraStatus{asset, ok|degraded|down}` (roads, power, water, telecom), `TriggerCascade{chain}`
- Resources: `AddResource`, `RemoveResource`, `SetResourceStatus{available|committed|unavailable}`
- Population: `SetPopulationState{sector,…}`, `SetBehaviourProfile`
- Exercise: `SetPhase`, `SendInject`, `ForceAction{role, Command}` (acts on a player's behalf)

**Player actions** (a subset; each role's manifest decides which of these it may use)
- `OrderEvacuation{area|facility, destination?}`, `IssueWarning{area, channel}`
- `CloseRoad{segment|area, civilian_only}`, `ReopenRoad`
- `DeployUnit{unit, task: attack|line|drop|standby, target}`
- `OpenShelter{site}`, `RequestResource{from_role|external, kind, qty}`
- `RequestExternalAssistance{capabilities, …}` (arrives after a mobilisation delay)
- `SendMessage{to_role, text}`, `RequestInfo{topic}` (no sim effect; routed through the exercise engine)

Each command has: `validate(&World, actor) -> Ok | Err(reason) | OkWithConstraints(notes)` and `apply(&mut World)`. Validation is deterministic: the unit exists and is free, the target is reachable, capacity is available, the actor has the authority.

---

## 5. Turn loop

```mermaid
stateDiagram-v2
    [*] --> Briefing
    Briefing --> Inject: Master opens turn
    Inject --> Response: injects sent (LLM-translated per role)
    Response --> Adjudication: all replied or deadline
    Adjudication --> Resolution: Master approves/edits/rejects proposals
    Resolution --> Update: sim advances Δt (Master-chosen)
    Update --> Inject: next turn
    Update --> Debrief: Master ends exercise
    Debrief --> [*]
```

| Phase | What happens |
|---|---|
| Inject | Master picks from the MSEL, writes an inject, or prompts the LLM for one. The LLM tailors and translates it for each recipient. Master may preview before sending |
| Response | Players read and reply in free text in their own language, and may message other permitted roles. The LLM parses each reply into a **Proposal**. If a reference is ambiguous ("send two teams there"), the LLM asks the player a clarification question instead of guessing |
| Adjudication | Master sees each proposal: original text, translation, parsed commands, sim validation result, LLM notes. Actions: approve / edit / reject / defer. Player gets feedback in their own language |
| Resolution | Approved commands are applied, then `advance(Δt)` runs. A checkpoint is saved. The big screen replays the turn accelerated |
| Update | Per-role situation update: a digest filtered by visibility, summarised and translated by the LLM. Master gets the full picture plus any flagged issues |

At any phase the Master can issue god-mode commands through the GUI or a prompt. Master prompts produce proposals too: they are shown for one-click confirmation, and nothing is applied silently.

### Proposal record
```
Proposal { id, turn, source: Player(role)|Master, original_text, lang,
           translation, commands: [Command], validation: [Result],
           llm_rationale, status: pending|approved|edited|rejected|deferred,
           master_note, applied_at_t }
```

---

## 6. LLM workflows

| # | Workflow | Input | Output |
|---|---|---|---|
| L1 | Inject adaptation | inject (working language), recipient role, player language | translated role-specific message |
| L2 | Response → actions | reply text, role manifest (allowed commands + controlled resources), role digest, gazetteer | tool calls restricted to the role's commands, or a clarification question |
| L3 | Master prompt → commands | prompt, full digest, full catalogue | tool calls (god mode) |
| L4 | Feedback | proposal decision + validation reasons | message to the player in their language |
| L5 | Situation update | role-filtered digest diff for the turn | short briefing in the player's language |
| L6 *(P2)* | Inject suggestion / cascade ideas | digest, exercise objectives | candidate injects/commands → Master |
| L7 *(P2)* | Debrief narrative | event log | timeline + discussion points |

Grounding: the sim exposes a compact **digest** (clock, fire summary per sector, facilities, units, resources, roads, infrastructure, pending arrivals) and a **gazetteer** mapping names and aliases in every language to entity ids, so "the school" resolves to `facility:school_a`. Tool schemas are generated from the `Command` enum, so the LLM cannot emit a command type that doesn't exist.

Providers: cloud model (primary) and Ollama (local fallback), configured the same way as the reference `chat` crate. All prompts and outputs are written to the event log.

---

## 7. Scenario package

One directory per scenario, baked offline (Python scripts as in the reference repo; nothing runs at game time):

- **Geography:** DEM, fuel raster, render terrain, OSM roads/buildings, sectors
- **Exposure:** synthetic population on buildings, facilities (schools, hospitals, fuel station, shelters) with occupancy
- **Infrastructure graph:** dependencies (fuel depot → power line → pumping station → water) used by `TriggerCascade`
- **Resources:** units and assets per owner role, base location, mobilisation delay, external assets with arrival delay
- **Roles:** `roles.yaml` defining name, level (national/regional/local), language, allowed commands, controlled resources, visibility filter, permitted message links
- **Gazetteer:** multilingual names/aliases → entity ids
- **MSEL:** using the Armenian MSEL fields (event id, inject id, time, sender, recipient, text, objective, expected response)
- **Initial state + optional checkpoints** (e.g. Day 3, Day 10)

---

## 8. Interfaces

**Master web UI:** 2D map (fire, units, facilities, roads) · clock and turn controls (open turn, deadline, resolve with Δt, skip time, checkpoint/rollback) · inject composer + MSEL list + send · proposal queue (approve/edit/reject) · god-mode panel (forms generated from the catalogue) · prompt box · event timeline.

**Player web UI (tablet):** role card · inbox (injects, messages, feedback) · role-filtered situation + small map · own resources · free-text reply box · message another permitted role. The UI never shows commands or AI terms to players.

**Observer/Evaluator UI (P2):** read-only timeline, inject → reply → decision → consequence chains, evaluator notes.

**Big screen (Bevy):** 3D/2D scene, turn/clock HUD, active injects, accelerated turn replay, anonymised decision → consequence captions.

**Protocol:** REST for actions, WebSocket for pushes (`turn`, `inbox`, `proposal`, `state`). Devices join by QR code / role token. LAN only; internet is used only by the LLM gateway.

---

## 9. Non-functional specs

- **Determinism:** seeded runs; step-size-invariant models (reference repo findings 5, 18); restoring a checkpoint gives the same state.
- **Performance:** resolving 1 h of simulated time should take ≤ 10 s on the stand machine; UI push latency < 1 s; 6 players + 3 evaluators + Master at once.
- **Traceability:** every inject, reply, proposal, decision, command and turn is timestamped (wall clock + sim clock) in the event log, and can be exported for the debrief (JSON/CSV → report).
- **Degradation:** if the LLM is unavailable, all commands can still be entered from the Master GUI and injects can be sent as written. The exercise never blocks on AI.
- **Deployment:** single binary + scenario directory + web assets. Runs on one GPU laptop with a dedicated router, and a 4G backup for the LLM.
- **Configurability:** roles, players, languages and resources come from data, never from code.

---

## 10. MVP scope & build order

1. `sim` over reused `fire`/`abm`: `Command` catalogue v1, `advance`, checkpoints, headless tests
2. `exercise` + `server` + minimal Master/Player UIs: a turn loop you can play manually (no LLM)
3. `llm`: L1 translation, L2 response parsing, L4 feedback; proposal queue in the Master UI
4. L3 Master prompts; god-mode panel generated from the catalogue
5. `viz` big-screen HUD + turn replay; Forum scenario package with MSEL
6. P2: observer/evaluator UI, L5–L7, cascades, debrief export

---

## 11. Open questions

- Forum scenario location (Spotorno, a place in a PPRD East country, or synthetic)?
- Stay on Bevy 0.14 (direct port from the reference repo) or upgrade?
- Working language and player languages for the Forum?
- Default Δt per turn, and how `SkipTo` Day 10 is produced (fast-forward the model vs. Master-painted state vs. pre-baked checkpoint)?
- Do players see a map, or only text plus the shared big screen?
- LLM provider for the Forum (cloud vs. local), given data sensitivity and connectivity?

---

## 12. As built — working draft v0.2 (2026-09-24)

The implementation follows this spec with the decisions and deviations below. The details are in `README.md` and `docs/`; the demo video is `docs/media/ttx_demo.mp4`.

### 12.1 Spec items

| Spec item | As built |
|---|---|
| Fire model (§1) | PROPAGATOR Rust core vendored in-process (`crates/propagator-core`), legacy 7 fuels, 20 m cells, **32-run Monte Carlo ensemble**. The fire is the set of cells reached by ≥ `cutoff` of the runs (default 0.5 = median fire; `--cutoff 0.25` for a pessimistic fire), with quantile arrival times; cells reached by fewer runs form an uncertainty halo. Ensemble-mean fireline intensity per cell. Spotting off |
| ABM (§1, §3) | Not yet. Buildings carry occupant counts (damage by exposure, evacuation with delays and buses, people-at-risk indicator); approved role deployments allocate units and travel from configurable staging bases before task effects; ground crews, air tankers, buses and patrols have simulation-clock travel/status and 3D group markers. Master physical interventions remain direct. Abstract straight-line training travel, not a road-routing ABM |
| Master (§1) | Omnipotent: map tools, commands by code (JSON), AI prompts (proposals with commands and inject drafts, confirmed before applying), turn length and turn **speed**, rollback |
| Players (§1) | Injects translated per player; free-text replies turned into commands by the LLM within the role's permitted actions; every player action needs Master approval; feedback and situation updates in the player's language |
| Architecture (§2) | One process: the server (Rocket, tokio) and the Bevy renderer share one `Arc<Mutex<Exercise>>`. The server mutates it; the renderer only `try_lock`s and reads. The lock is never held across LLM calls. Channels were not needed |
| Crates (§3) | `propagator-core`, `scenario`, `fire`, `sim`, `llm`, `exercise`, `server`, `frontend`, `viz`, `app` (binary `ttx`); `web/dist/` is the generated Yew/WebAssembly browser bundle. `telemetry` is a JSONL event log inside `exercise` (`runs/run-*.jsonl`) |
| Command catalogue (§4) | 15 commands (9 player, 6 Master-only), with deterministic validation (authority, resources including already-approved actions, places, capacities). Locations by zone code of an 8×8 grid (`F4`), zone edge (`F4:N`), named place/facility, or coordinates. See `docs/commands.md` |
| Turn loop (§5) | `Briefing → Orders → Resolving → Orders … → Debrief`. Player actions apply at resolution, Master actions immediately. The turn is **simulated live in 1-second steps** at the Director's speed factor (0 = maximum, ~0.45 s for 30 min; 120× = 15 s). The 3D view redraws continuously; browsers refresh at up to 4 Hz. Snapshots per turn with rollback |
| LLM workflows (§6) | OpenRouter, `deepseek/deepseek-v4.1-flash`. L1–L5 implemented (translate, parse reply with clarification and role messages, Master prompt with inject drafts, feedback, situation update). **Each player and the Director has one continuous conversation** with the AI (what they received, what they wrote, the AI's interpretations), sent as chat history, so follow-ups keep their context. Record/replay cassette (`TTX_LLM_CASSETTE`) for reproducible sessions. L6/L7 (suggestions, debrief narrative) not yet |
| Scenario package (§7) | One JSON file from a deterministic synthetic generator (Valverde valley, 4 × 4 km): terrain, fuel, 266 buildings, roads, sectors, zone grid, multilingual gazetteer, 3 roles, reinforcement offers, MSEL of 8 injects |
| Interfaces (§8) | Rust/Yew WebAssembly conversational EXCON and player pages, canvas maps, advanced console and read-only evaluator. **`play.html`** opens EXCON; players use their individual capability links. Big screen in Bevy (below) |
| Non-functional (§9) | Deterministic seeded model; fire result independent of step size; 111 headless tests with a mock LLM plus a live OpenRouter test; works with no LLM (English injects, manual command entry) |

### 12.2 Beyond the original spec

| Area | As built |
|---|---|
| 3D rendering (`viz`) | Terrain with Blender vegetation (from `propagator_abm`); flame tongues sized by Byram flame length of the ensemble-mean intensity and scaled by run probability, dim halo tongues below the cutoff, wind tilt; ground cooling from flame-front orange to charcoal; wind-driven smoke plume (wind increasing with height, gusts); sparks |
| Camera | Cinematic director: shots of the place each inject, proposal, order or Director command refers to; follows the fire while a turn is simulated; wide establishing orbit in between; spring-smoothed, terrain-aware; manual override |
| Lighting | Low warm late-afternoon sun with cascaded shadows, fog, ACES tone mapping, HDR bloom, flickering fire point lights; smoke and flames cast no shadows |
| Remote play | Private Microsoft dev tunnel (`devtunnel host -p 8080`) to play `play.html` from a phone |
| Demo recording (`scripts/record`) | Scripted session in Chrome (Playwright) with every role, next to the simulator window streamed into ffmpeg while recording; Gemini TTS narration (`google/gemini-3.1-flash-tts-preview` via OpenRouter, every clip checked by transcription); burned-in subtitles plus `.srt`/`.vtt`; LLM answers replayed from a committed cassette; about 5 min to re-record a 4 min video. README demo tables generated from the recording |
| Instrumentation | Screenshots (`P`, `runs/shot.req`), stream recording (`runs/rec/ON`), `TTX_CINEMATIC`, `TTX_CAM`, `--realizations`, `--cutoff` |

### 12.3 Open questions (§11): status

| Question | Status |
|---|---|
| Forum scenario location | Open: the synthetic Valverde valley stands in; real places can be baked with the `propagator_abm` pipeline |
| Bevy version | Resolved: stayed on 0.14 (+ bevy_egui 0.28) to reuse `propagator_abm` code and assets |
| Working and player languages | Resolved: English working language; each player picks any language at runtime (the demo uses Italian, English and Armenian) |
| Default Δt and time jumps | Resolved: 30-minute turns by default, set per resolution; time jumps run the model forward (`AdvanceTime`) |
| Do players see a map? | Resolved: yes, first on the player page |
| LLM provider | Resolved for the draft: OpenRouter (cloud), with a cassette for offline replay; a local model is not wired in yet |

### 12.4 Next steps

The list of open work, including the v0.3 draft specs (one file per item, with proposed priorities in [`docs/specs/`](docs/specs/README.md)), is in [`docs/todo.md`](docs/todo.md).

# PPRD East 4 agentic TTX — project guide

Turn-based wildfire table-top exercise: Rust simulator (PROPAGATOR core + buildings) with a Bevy big screen, a web Master console, web player pages, and an OpenRouter LLM that mediates. Start with `README.md`; the design docs are in `docs/`.

## Decisions (settled with Mirko)

| Topic | Decision |
|---|---|
| Simulator | Rust + Bevy 0.14 (+ bevy_egui 0.28), from scratch; `propagator-core` vendored from `~/dev/experiments/propagator_abm` |
| Game | Turn-based. Master omnipotent (GUI tools, AI prompts, commands by code). Every player action needs Master approval |
| People | Not modelled individually: buildings have occupant counts |
| Suppression | Approved role deployments travel from a configurable staging base; physical task effects wait for arrival. Master interventions act directly. See docs/api.md. |
| Places | Named places, zone codes of an 8×8 grid (`F4`), zone edges (`F4:N`), or coordinates |
| LLM | OpenRouter, default `deepseek/deepseek-v4.1-flash`, key in `.env`; must degrade gracefully |
| Scenario | Synthetic "Valverde valley" for now |
| 3D assets | Reuse `propagator_abm` Blender meshes (`assets/models/meshes.json`) |
| Turn resolution | Simulated live between turns in 1-second steps. The 3D view updates continuously; browsers are coarser (≤ 4 Hz). Default speed = maximum (`speed` 0); the Director sets a factor (e.g. 60× = 1 simulated min/s) |
| Demo video | Narrated with OpenRouter TTS `google/gemini-3.1-flash-tts-preview` (voice Charon), telling the story of the incident (not game mechanics); every clip checked by transcription; subtitles burned in + track + .srt/.vtt; LLM answers replayed from `scripts/record/llm_cassette.jsonl`; simulator streamed into ffmpeg while recording |
| AI context | One continuous conversation per player and for the Director (received messages, replies, interpretations) |
| Player UI | Situation map first on the player's page (phones) |
| Camera | Cinematic director follows injects, proposals, orders and the fire during turns; sun, shadows, fog, bloom, fire lights |
| Flame rendering | Byram flame length from ensemble-mean intensity, scaled by probability; halo tongues below the cutoff; Froude wind tilt; age-based ground glow; smoke/sparks. Build on `propagator_abm` ideas but keep ensemble-aware |
| Fire ensemble | 32-run Monte Carlo; the "actual" fire is cells reached by ≥ `cutoff` of runs (default 0.5, 0.25 = pessimistic), with quantile arrival times; lower-probability cells shown as a halo |

## Working agreements

- Only `sim::World::apply` changes the world, and only with a `Command`. The LLM returns JSON that is deserialised and validated. It never drives the sim directly.
- Never hold the exercise mutex across `.await` (see `server/src/flow.rs`). The renderer only `try_lock`s and reads.
- Model code (`scenario`, `fire`, `sim`, `exercise`, `llm`) stays free of Bevy and HTTP and is tested headless.
- Integrate over simulated time, never per call (step-size invariance).
- `vegetation_changes` is NaN-sparse. Suppression acts only on unburnt fuel. Single-cell ignitions fizzle, so use ≥ 40 m.
- Keep docs in `docs/` and tests in step with code changes.

## Commands

```bash
cargo test --release --workspace
TTX_LIVE_LLM=1 cargo test -p llm --test live -- --nocapture
cargo run --release -p app [-- --headless] # :8080, master.html / player.html
cargo run -p scenario --bin gen-scenario
./scripts/demo_session.sh                  # scripted 2-turn session against a running ttx
echo "$PWD/runs/x.png" > runs/shot.req     # simulator window screenshot (or press P)
./scripts/record/record.sh                 # re-record the narrated demo (own ttx on :8090, LLM cassette, TTS cache)
python3 scripts/record/readme_demo.py      # refresh the docs/demo.md timeline/outcome from the recording
devtunnel host -p 8080                     # private tunnel for phone play (play.html)
```

## Operational model (ABM-derived)

- `--operational-model` or scenario `operations` opts into road routes, progressive suppression, aggregate vehicle traffic, intensity exposure and communications. Legacy scenarios retain the calibrated rules. Spotting is explicitly authored and defaults off.
- The graph is static geometry; mutable routes, work, water, queues, site failures and pending warnings live in `sim::Dyn` for rollback. Keep all model decisions in `sim`; Bevy only draws snapshots.
- `DeployEngine` needs an approved role allocation. Ground routes obey fire cuts; civilian traffic also obeys police closures. No path means waiting/failure, never a straight-line fallback.
- EU12 ids differ from legacy ids: use scenario fuel definitions for burnability, fuel removal and rendering.
- Scenario Factory port and import commands, provenance, training assumptions and verification: `docs/operational-model.md`.

## Roccavento lessons update (2026-10-20)

- Reusable skills: `docs/ttx-skills.md`, under `.claude/skills/`. Read relevant ones;
  explicit user authorization takes precedence over source-game checkpoints.
- Current transfer and limits: `docs/operational-upgrade.md`. Segment closures,
  terrain travel, emergency queue delay and local autonomy are enabled by
  `--operational-model`, independently configurable in scenario `operations`.
- Keep approved mission centre separate from current post/base. Local policies
  never grant resources or change approved fire-line endpoints. Suspended tasks
  retain physical allocations and respect confirmed returns.
- Forecasts are private EXCON drafts computed from owned checkpoints outside the
  exercise mutex. Publish only after review to selected roles; reject obsolete
  generations and invalidate role products on rollback.
- `scripts/lab/run.sh SUITE NEW_OUTPUT` runs paired seeds through real commands;
  `scripts/lab/generate_cases.py NEW_DIRECTORY` creates explicit synthetic fixtures.
- `TTX_QUALITY=projector|low|high` changes presentation only. Invisible fixtures:
  `docs/graphics.md`, `scripts/snapshot.sh`, `TTX_SNAPSHOT_SPEC`. Never start the
  live HTTP/LLM path when generating a snapshot. Inspect every generated image.
- Avoid formatting unrelated modules: use scoped rustfmt with `skip_children=true`.


- Batch visual states: `scripts/snapshot.py --out NEW_DIRECTORY --spec 'name=fire,t=30,at=fire,ui=0,marks=0'`.
  Each process uses real fixture approvals, no HTTP/LLM; names cannot reuse stale PNGs.
  Angles in this batch format are degrees; legacy `TTX_CAM` stays radians.
  `--bench 3` freezes animation after settling and reports wall/CPU/render/GPU samples.
  Read `docs/graphics.md`; hidden-window FPS is not an interactive projector guarantee.

- Fire display defaults to the continuous, read-only field; `V` / `TTX_FIRE_RENDER=cells`
  selects exact-cell presentation. Snapshot states accept `fire=continuous|cells`.
  Physical decisions stay on the model grid; preserve cleared-cell masking and
  keep ensemble uncertainty separate from confirmed-fire reconstruction.

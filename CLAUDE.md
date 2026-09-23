# PPRD East 4 agentic TTX — project guide

Turn-based wildfire table-top exercise: Rust simulator (PROPAGATOR core + buildings) with a Bevy big screen, a web Master console, web player pages, and an OpenRouter LLM that mediates. Start with `README.md`; the design docs are in `docs/`.

## Decisions (settled with Mirko)

| Topic | Decision |
|---|---|
| Simulator | Rust + Bevy 0.14 (+ bevy_egui 0.28), from scratch; `propagator-core` vendored from `~/dev/experiments/propagator_abm` |
| Game | Turn-based. Master omnipotent (GUI tools, AI prompts, commands by code). Every player action needs Master approval |
| People | Not modelled individually: buildings have occupant counts |
| Suppression | Fire lines and water drops act directly; no simulated teams |
| Places | Named places, zone codes of an 8×8 grid (`F4`), zone edges (`F4:N`), or coordinates |
| LLM | OpenRouter, default `deepseek/deepseek-v4.1-flash`, key in `.env`; must degrade gracefully |
| Scenario | Synthetic "Valverde valley" for now |
| 3D assets | Reuse `propagator_abm` Blender meshes (`assets/models/meshes.json`) |
| Turn resolution | Simulated live between turns in 1-second steps. The 3D view updates continuously; browsers are coarser (≤ 4 Hz). Default speed = maximum (`speed` 0); the Director sets a factor (e.g. 60× = 1 simulated min/s) |
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
./scripts/record/record.sh                 # re-record docs/media/ttx_demo.mp4 (own ttx on :8090)
devtunnel host -p 8080                     # private tunnel for phone play (play.html)
```

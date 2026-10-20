# Operational graphics and reproducible captures

The renderer reads snapshots and never makes model decisions. The native
pipeline now uses the latest `propagator_abm` PBR materials, including the source
working-tree changes to sky reflections and unit finishes. Input revision and
SHA-256 hashes are in `assets/models/pbr-provenance.json`.

Terrain has its own PBR material: procedural dry meadow, ground colour and
surface normals, signed-distance asphalt/tracks, kerbs and rock strata on steep
faces. Normal textures retain the supplied visual DEM resolution independently
of mesh detail. Roads come from the actual scenario network. Fire, burnt fuel,
clearing, wetting and ensemble uncertainty are a separate exact-cell texture;
uncertain cells do not ignite plants. Fire cuts are drawn on the affected graph
segments; police closures remain a separate administrative state.

Foliage uses the source vertex, depth and shadow shaders: bark, leaf clumps,
roughness and diffuse transmission. Fifteen supplied pine, chestnut, scrub and
olive varieties are planted deterministically, with several plants per forest
cell and visual thinning along roads. The GPU reads one state/ignition-time
texel per plant. Meshes remain immutable when fire advances or rolls back.
The crown chars and burns away over 240 seconds from model arrival; the same
trunk stays in place and the matching fine branches/debris emerge. Fuel removal
hides both layers, including their shadows. This visual crown duration does not
change the model's fuel residence time or suppression effectiveness.

Structures use the imported procedural plaster, roof-tile and surface finishes.
Rectangular dwellings now have varied plaster, plinths, eaves, hipped roofs,
windows, shutters and doors; school, town hall, fuel station and industrial
facilities use authored landmark meshes. The optional `Building.visual_kind`
retains Factory artwork categories: plazas, parking, pitches and assembly areas
remain draped open ground, and churches/fire stations use dedicated kit models.
This metadata does not change operational kind, occupants or shelter capacity;
existing scenarios without it keep the kind-based renderer. Footprints remain
the rectangular representations supplied by the TTX scenario format.
Approved deployments use the latest
articulated engine, tanker and firefighter bakes, with PBR surface identifiers
for paint, metal, rubber, glass, lamps, skin, fabric, reflective strips and
leather. Wheels/propellers/limbs use the authored pivots and simulator time.
Buses, patrols and aggregate civilian traffic use the town vehicle kit. Symbols
remain magnified for map visibility; one model depicts one aggregate deployment,
not a newly simulated person or vehicle. Actual positions, routes and statuses
still come from the model. Hollow diamonds indicate destinations and rings
indicate task status without repainting the red engine. A ground task without
a valid route has no invented straight path.

A procedural sky, time-dependent sun, fog, HDR tone mapping and diffuse/specular
sky cube maps light all PBR surfaces consistently. The lighting assumes 44.2°N
and a representative summer day; scenario clock hours are treated as local solar
time for presentation. It is not a georeferenced solar or weather prediction.
`TTX_START_HOUR` overrides the initial lighting hour for visual review.

Map labels use opaque backing, above-canopy anchors and deterministic screen
spacing with leaders. On the web, vehicle markers use only the deployments
included in the authorized view. Forecast outlines remain dashed purple,
labelled with issue time and horizon. Only products supplied by EXCON in that
authorized view can appear. The browser situation map remains a 2D operational
map; this PBR pipeline is the native Bevy big screen.

`TTX_QUALITY=projector` (default) uses shadows, 4× MSAA, little bloom, 65% of
forest density, 4.5 km vegetation visibility and two local fire lights. `low`
uses 35%, 2.5 km, SMAA Medium, no shadows, bloom or fire lights; `high` uses full
density, 9 km, 4× MSAA, a 4096 shadow map and eight lights. These are presentation
presets, not performance guarantees. Distant vegetation is culled by chunk;
fuel and fire on the ground remain. The optional `render_terrain` heightfield
carries fine visual relief without changing the physical grid or travel slopes.
Projector/low sample every second/fourth supplied mesh node. Buildings, roads,
camera and shading share the continuous visual height sampler.

The camera is stable by default. `A` toggles the optional cinematic director,
`F` frames fire, `R` or Home restores the initial view. Keyboard shortcuts yield
to text entry. Manual views are kept above terrain. `TTX_CAM=x,y,distance,yaw,pitch`
fixes the frame in world metres and radians.

## Invisible snapshot harness

```sh
# Builds and takes three stationary views, without showing or focusing a window.
sh scripts/snapshot.sh runs/visual-review --realizations 4

# A real approved deployment, through the Exercise proposal/approval/execution flow.
TTX_SNAPSHOT=runs/visual-review/deployment.png \
TTX_SNAPSHOT_SPEC=scripts/visual-fixtures/deployment.json \
TTX_CAM=1600,1300,800,0.6,0.62 \
target/release/ttx --operational-model --seed 1 --realizations 4

# A later fire state without HUD.
TTX_SNAPSHOT=runs/visual-review/fire.png TTX_SNAPSHOT_UI=0 \
TTX_SNAPSHOT_SPEC=scripts/visual-fixtures/fire.json \
TTX_CAM=2800,2700,1500,0.7,0.5 \
target/release/ttx --seed 1 --realizations 4
```

A fixture accepts `commands` (validated EXCON interventions), `role_orders`
(entries with `role` and `commands`, approved and executed with real resource
accounting), `minutes` (normal turn resolution), and optional `rollback_to`.
Invalid orders fail instead of fabricating states. No HTTP server or LLM is
started in snapshot mode. Each invocation reloads the scenario and seed.
`TTX_SNAPSHOT_MARKS=0` hides the grid/route/closure overlays for scene-only captures.
`TTX_SNAPSHOT_SETTLE` defaults to 3 virtual seconds at fixed 60 Hz, making
animated fire and smoke repeatable for the same state and camera. The process
exits only after the GPU callback writes the PNG and reports save failures.

Always inspect each output, including oblique and overhead views. These captures
verify appearance, not projector performance or operational validity. Measure
performance on the target EXCON machine; no FPS guarantee is implied by a profile.

## PBR regression check

`sh scripts/qa/pbr.sh runs/pbr-review/verified` builds offline and captures an
engine, the three approved unit types, the town, a 30-minute fire, the overhead
HUD and a low-quality rollback. It fails on GPU/shader errors as well as process
failures: a saved PNG alone is not sufficient. Inspect every generated PNG.
Input fixtures are in `scripts/visual-fixtures`; additional application arguments
can follow the output directory. The supplied views target the synthetic valley.

## Browser workflow check

After installing the existing recorder dependencies and building `app`, run
`node scripts/qa/forecast.cjs`. It opens Chrome headless, starts its own temporary
server with the LLM disabled, computes a private draft, inspects the preview and
publishes after explicit review to one selected role. It records desktop/mobile
panel images and fails on JavaScript page errors; the server is stopped afterward.
This is a functional regression check, not a first-time human usability study.


Verified on 2026-10-20: 202 workspace tests (including one doctest), native
release build and six GPU regression captures inspected at 3200×1800 on Apple
M4 Pro / macOS 26.6.2, plus a high-profile 1601×1601-node imported visual DEM.
The capture logs contain no shader/GPU errors. These are appearance and
regression checks; interactive frame times on the EXCON projector machine have
not been measured. Captures: `runs/pbr-review/verified/`; source inputs and
licence: `assets/models/pbr-provenance.json`.


## Flame renderer transferred from the latest game

`fire_shader.rs`, `fire_textures.rs` and the flame, smoke, fire-ground and
scene-light shaders now use the actual game code. `flames.rs` uses its
seven-segment travelling-wave ribbons, procedural masks, round cooling embers,
CPU-evaluated puff fog and ground glow/ash treatment. The source inputs' hashes
and fire revision are recorded in `pbr-provenance.json`.

The TTX keeps ensemble intensity/probability, its existing flaming lifetime and
wind tilt. Ground heat/coverage is bilinear for shading but clipped to confirmed
burning/burnt parent cells; a halo or fuel break gets no glow overlay. Decorative
embers do not ignite fuel or draw unconfirmed landing flashes. These visual
particles never alter `World`. Smoke/embers clear when time or turn rolls back;
dynamic meshes bypass stale frustum bounds. Additive flame RGB is multiplied by
its coverage (alpha zero) to prevent luminous ribbon triangles and dark rims.

## Batch snapshots and source rendering probes

The source `kiosk/snap.rs` state-list/camera workflow is adapted in
`scripts/snapshot.py` and `crates/viz/src/snapshot.rs`. Each state starts a fresh
TTX process, including earlier times, so approvals, allocations and particles
cannot leak across states. Fixtures still use the actual EXCON/proposal flow.
The source `bench.rs` CPU, render-thread and GPU timestamp probes are copied.

```sh
python3 scripts/snapshot.py --out runs/capture-new \
  --spec 'name=fire,t=30,at=fire,dist=1500,yaw=40,pitch=30,ui=0,marks=0;name=before,t=0,at=home'

# Frozen-state performance sample plus side-by-side report against an older run.
python3 scripts/snapshot.py --out runs/capture-bench --bench 3 \
  --compare runs/capture-new --spec 'name=fire,t=30,at=fire,ui=0,marks=0'
```

States are separated by `;`, assignments by `,`. Keys:

| Key | Meaning |
|---|---|
| `name` | Unique output basename, letters/numbers/underscore/hyphen |
| `t` | Whole simulated minutes from start; overrides fixture `minutes` |
| `fixture` | JSON file with real commands, approved role orders and optional rollback |
| `at` | `home`, `fire`, `place:NAME`, `task:ID`, or `X:Y` metres |
| `dist` | Camera distance in metres, minimum 20 |
| `yaw`, `pitch` | Degrees; pitch magnitude 3–89, unlike legacy `TTX_CAM` radians |
| `ui`, `marks` | `0` hides, `1` shows |
| `quality` | `projector`, `low` or `high` |
| `settle` | Virtual settling seconds, default 3 |
| `bench` | Real seconds to benchmark after settling, default 0 |

Global options include scenario, seed, realizations, `--size 1600x1000`,
`--scale 1`, `--legacy-model`, `--no-build` and `--timeout`. A fresh directory is
required when names already exist; unknown keys and invalid values fail. No
HTTP server or LLM is started. The runner checks process status and shader/GPU
logs, writes each resolved state/fixture and a scenario hash to the manifest,
and produces `report.html`; `--compare` adds the older images alongside.

Native equivalents: `TTX_SIZE`, `TTX_SCALE`, `TTX_SNAPSHOT_AT`,
`TTX_SNAPSHOT_DISTANCE`, `TTX_SNAPSHOT_YAW`, `TTX_SNAPSHOT_PITCH` and
`TTX_SNAPSHOT_BENCH`. Exact camera hold runs after normal controls. During a
benchmark animation freezes after settling, vsync is requested off, and timing
uses wall intervals, not the fixed 60 Hz presentation clock. The JSON alongside
the PNG contains frame/main/encode/render/GPU mean and p95 with sample counts.
Unavailable probes are `null`. Hidden-window FPS can be compositor limited and
GPU time clock limited: compare identical states, resolution and hardware.

Final verification: **205 Rust workspace tests**, four Python parser tests,
release build, native batch captures and GPU timestamps exercised on Apple M4
Pro/macOS 26.6.2. Inspected fire, close fire, overhead UI and low-profile rollback;
no shader/GPU errors. The one-second 1600×1000 fire sample is a harness smoke
test, not a projector performance qualification. Local artifacts:
`runs/flames-review/` and `runs/snapshot-port-review/`.

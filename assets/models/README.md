# Emergency simulation models

Blender mesh bakes and generators transferred from `propagator_abm` under the
existing EUPL-1.2 attribution.

- `emergency_assets.blend`: seven named collections (pedestrian, firefighter, car, fire_engine, pine, oak, bush). Each collection is authored at the origin; isolate a collection to edit it.
- `meshes.json`: baked positions, normals, vertex colors, triangle indices and bark masks, embedded in the native and web game.
- `preview.png`: Blender studio render. The fire engine is shown with red paint; legacy symbols use operational status tint; the latest units retain their authored paint with separate operational rings.

Regenerate from the repository root:

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --python scripts/build_models.py
cargo test -p viz models::tests
```

The generator is the source of truth: regeneration rebuilds the `.blend` and mesh bake. Make persistent design changes in `scripts/build_models.py`.

Blender coordinates are metres, Z up, front -Y; baked meshes use Y up and front +Z. People and vehicles have their origins at ground level. Trees are approximately one metre tall before the vegetation system applies species-specific sizes. Bushes are normalized clumps. Existing map-symbol scale factors still apply.

People and suppression models bake neutral vertex tones so the existing status materials stay legible. Cars retain colored details. Vegetation retains species palettes and authored crown shading; bark masks distinguish stems from leaves. Plant meshes are welded, limited to 120 vertices / 160 triangles per archetype, and merged into existing spatial chunks so burning and culling keep working. Grass remains procedural.

Triangle budgets: pedestrian 356, firefighter 388, car 712, fire engine 1,688, pine/oak 152 each, bush 100. More detailed vegetation increases triangle counts versus the former procedural models; draw-call batching and the existing browser density reduction are preserved.

## Additional urban kit

`town.json` is adapted from `/Users/mirko/dev/experiments/propagator_abm/assets/models/town.json` (EUPL-1.2). School, town hall, fuel station and industrial buildings use these landmark meshes; rectangular buildings use detailed procedural architecture and all status colors come from simulation state. Meshes use metres, Y up, and RGB vertex colors converted to RGBA by the loader.

The reproducible generator is `scripts/build_town_models.py`; run it with Blender from the repository root. It also writes the editable `.blend` and a preview.

## Vegetation varieties (2026-10-20)

`vegetation.json` and `scripts/build_vegetation_models.py` are transferred from
`propagator_abm` under the existing EUPL-1.2 provenance. Fifteen varieties (four
pines, four chestnuts, four scrub forms and three olives) share live stems and
matching fine-branch/debris additions. The TTX uses the source GPU pipeline:
one RG32Float texel per plant encodes state and ignition time; the vertex,
fragment, depth and shadow passes retain stems, consume foliage and reveal the
matching additions. Clearing and rollback derive from the authoritative snapshot
without rewriting meshes. The source generator remains reproducible with
Blender; this transfer did not regenerate its bake.

SHA-256 of the transferred inputs:

- vegetation.json: `39d78204a1b40008972eef9a23129027a2430760f10a5478fab03b71c34a66e3`
- generator: `cc59770aa04fb98417e304219cb0789ae3cd718c602bb2d78bba24357b741a61`

## Articulated PBR units (latest source, 2026-10-20)

`units.json` and `scripts/build_unit_models.py` contain the engine, tanker and
firefighter bodies and their wheel, beacon, propeller and limb parts. Their
`surfaces` attribute is encoded as vertex alpha (`id / 255`, 1 means default)
for the structure shader. This is a material identifier, not transparency.
Each `rig` gives its authored pivot, axis, animation kind and phase. Preserve
these fields when editing/regenerating; RGB-only loading loses the PBR response.
Use Blender in background mode with the generator to rebuild the assets.
The TTX uses metre-authored meshes with presentation magnification, as described
in `docs/graphics.md`. Physical allocations and movement remain in `sim`.

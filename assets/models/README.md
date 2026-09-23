# Emergency simulation models

Original, editable Blender assets generated for this project; no external art or textures.

- `emergency_assets.blend`: seven named collections (pedestrian, firefighter, car, fire_engine, pine, oak, bush). Each collection is authored at the origin; isolate a collection to edit it.
- `meshes.json`: baked positions, normals, vertex colors, triangle indices and bark masks, embedded in the native and web game.
- `preview.png`: Blender studio render. The fire engine is shown with red paint; game symbols use their live operational status tint.

Regenerate from the repository root:

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --python scripts/build_models.py
cargo test -p game models::tests
```

The generator is the source of truth: regeneration rebuilds the `.blend` and mesh bake. Make persistent design changes in `scripts/build_models.py`.

Blender coordinates are metres, Z up, front -Y; baked meshes use Y up and front +Z. People and vehicles have their origins at ground level. Trees are approximately one metre tall before the vegetation system applies species-specific sizes. Bushes are normalized clumps. Existing map-symbol scale factors still apply.

People and suppression models bake neutral vertex tones so the existing status materials stay legible. Cars retain colored details. Vegetation retains species palettes and authored crown shading; bark masks distinguish stems from leaves. Plant meshes are welded, limited to 120 vertices / 160 triangles per archetype, and merged into existing spatial chunks so burning and culling keep working. Grass remains procedural.

Triangle budgets: pedestrian 356, firefighter 388, car 712, fire engine 1,688, pine/oak 152 each, bush 100. More detailed vegetation increases triangle counts versus the former procedural models; draw-call batching and the existing browser density reduction are preserved.

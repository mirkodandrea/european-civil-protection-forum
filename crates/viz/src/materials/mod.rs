//! The scenery's materials, one type per kind of surface.
//!
//! - [`TerrainMaterial`]: the ground, its roads and meadows (`terrain.wgsl`).
//! - [`FoliageMaterial`]: the vegetation, and its fire (`foliage.wgsl`, with
//!   the vertex stages `veg_vertex.wgsl` / `veg_prepass.wgsl`).
//! - [`StructureMaterial`]: buildings, landmarks, boats, rocks and the units:
//!   StandardMaterial plus a procedural [`Finish`] and a PBR response per part
//!   (`surface_alpha`, baked per vertex) (`structure.wgsl`).
//! - Everything else (pins, rings, jets, fields) is a plain `StandardMaterial`.
//!
//! All of them are lit by Bevy's PBR (`apply_pbr_lighting`) and fogged by it.
//! Terrain and foliage are materials of their own rather than extensions of
//! StandardMaterial because of WebGL2: it has 16 texture units per stage, and
//! every texture in a pipeline's layout takes one whether the shader reads it or
//! not. StandardMaterial's six would leave the terrain no room for its four.
//! The shared surface functions are in `scenery.wgsl` (`rocca::scenery`).

mod foliage;
mod structure;
mod terrain;

pub use foliage::{FoliageMaterial, ATTRIBUTE_PLANT};
pub use structure::{structure, surface_alpha, Finish, StructureMaterial};
pub use terrain::TerrainMaterial;

use bevy::asset::load_internal_asset;
use bevy::prelude::*;

const NOISE: Handle<Shader> = Handle::weak_from_u128(0x2D6A_C0F3_5E19_4B87_8A41_7C03_E9B5_1D62);
const SCENERY_SHADER_HANDLE: Handle<Shader> =
    Handle::weak_from_u128(0x91A7_44D2_6B0E_4C71_AA90_2201_88C4_2F14);

pub struct MaterialsPlugin;

impl Plugin for MaterialsPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, NOISE, "../shaders/noise.wgsl", Shader::from_wgsl);
        load_internal_asset!(
            app,
            SCENERY_SHADER_HANDLE,
            "../shaders/scenery.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins((
            terrain::TerrainPlugin,
            foliage::FoliagePlugin,
            structure::StructurePlugin,
        ));
    }
}

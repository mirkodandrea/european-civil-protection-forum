//! Structures (`shaders/structure.wgsl`): StandardMaterial, so a model brings
//! its own colours, maps and alpha, plus a procedural surface finish.

use bevy::asset::load_internal_asset;
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
};
use bevy::prelude::*;
use bevy::render::mesh::MeshVertexBufferLayoutRef;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderRef, SpecializedMeshPipelineError,
};

const SHADER_HANDLE: Handle<Shader> =
    Handle::weak_from_u128(0x91A7_44D2_6B0E_4C71_AA90_2201_88C4_2F16);

/// Surface finish of a structure, drawn over its base colour in world (or
/// object) metres. A shader def each: one pipeline per finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[allow(dead_code)]
pub enum Finish {
    /// StandardMaterial as Bevy draws it.
    #[default]
    None,
    /// Town buildings, kit props and the pier: plaster, tiles, planks.
    Town,
    /// Landmarks: dressed stone.
    Stone,
    /// Boats: planking, deck boards, sail panels.
    Boat,
    /// Bare limestone off the terrain (the rocca's crag, the coast's boulders):
    /// the terrain's own beds, cut from world height.
    Rock,
}

impl Finish {
    fn shader_def(self) -> Option<&'static str> {
        match self {
            Finish::None => None,
            Finish::Town => Some("FINISH_TOWN"),
            Finish::Stone => Some("FINISH_STONE"),
            Finish::Boat => Some("FINISH_BOAT"),
            Finish::Rock => Some("FINISH_ROCK"),
        }
    }
}

/// What a baked vertex is made of, by id, for its PBR response (`SURFACE_*`
/// and `apply_surface` in `structure.wgsl`; the bakes' `SURFACES`, e.g.
/// `scripts/build_unit_models.py`): 1 paint under a clear coat, 2 metal,
/// 3 rubber, 4 glass, 5 lamp (glows), 6 skin, 7 cloth, 8 retroreflective tape,
/// 9 leather, 10 wood, 11 louvred shutters; 0 none, the material as it is.
///
/// The vertex colour alpha that carries surface `id` (1.0 = none). The
/// structure materials are opaque, so the alpha is free.
pub fn surface_alpha(id: u8) -> f32 {
    if id == 0 {
        1.0
    } else {
        id as f32 / 255.0
    }
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
#[bind_group_data(Finish)]
pub struct StructureFinish {
    pub finish: Finish,
}

impl From<&StructureFinish> for Finish {
    fn from(f: &StructureFinish) -> Self {
        f.finish
    }
}

pub type StructureMaterial = ExtendedMaterial<StandardMaterial, StructureFinish>;

impl MaterialExtension for StructureFinish {
    fn fragment_shader() -> ShaderRef {
        SHADER_HANDLE.into()
    }

    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let (Some(def), Some(fragment)) = (
            key.bind_group_data.shader_def(),
            descriptor.fragment.as_mut(),
        ) {
            fragment.shader_defs.push(def.into());
        }
        Ok(())
    }
}

/// A structure material: `base` with `finish` drawn over it.
pub fn structure(base: StandardMaterial, finish: Finish) -> StructureMaterial {
    StructureMaterial {
        base,
        extension: StructureFinish { finish },
    }
}

pub(super) struct StructurePlugin;

impl Plugin for StructurePlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            SHADER_HANDLE,
            "../shaders/structure.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins(MaterialPlugin::<StructureMaterial>::default());
    }
}

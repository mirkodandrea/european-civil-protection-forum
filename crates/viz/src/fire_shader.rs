//! Custom shader materials for the fire.
//!
//! Everything in [`crate::flames`] decides *where* fire geometry goes —
//! flame tongue placement, tongue count, the ground overlay's glow band. What
//! a `StandardMaterial` cannot do is make that geometry look like it is
//! burning rather than merely lit: a billboard with a static texture reads as
//! a sprite the moment the camera holds still. These two materials add a
//! per-pixel animated domain warp and flicker, driven by `globals.time` and
//! world position, so flame edges roil and the ground's glow band breathes
//! even between the overlay's own ~150 ms rebuilds.

use bevy::asset::load_internal_asset;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::mesh::MeshVertexBufferLayoutRef;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderRef, SpecializedMeshPipelineError,
};

const FLAME_SHADER_HANDLE: Handle<Shader> =
    Handle::weak_from_u128(0x9F3D_2C61_88AA_11EE_A0B1_2233_4455_6677);
const SMOKE_SHADER_HANDLE: Handle<Shader> =
    Handle::weak_from_u128(0x6C02_5D4F_93B1_47A8_8E2D_0F19_A3C7_B456);
const GROUND_SHADER_HANDLE: Handle<Shader> =
    Handle::weak_from_u128(0x4B7E_119A_2CDD_44FF_9021_AABB_CCDD_EEFF);

/// Flame tongues and embers: a textured, additive billboard whose lookup UV
/// is warped by animated noise. See `shaders/flame.wgsl`.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct FireMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub texture: Handle<Image>,
    /// x = 1 for flame tongues (rising, eroding, heat ramp); 0 for embers,
    /// which keep the plain warped-texture look.
    #[uniform(2)]
    pub mode: Vec4,
}

impl Material for FireMaterial {
    fn fragment_shader() -> ShaderRef {
        FLAME_SHADER_HANDLE.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }

    fn specialize(
        _pipeline: &MaterialPipeline<Self>,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Billboards are built double-sided (see QuadBuilder) precisely so
        // they read from any camera angle.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// Smoke puffs: unlit, alpha-blended, fogged by hand. See `shaders/smoke.wgsl`.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct SmokeMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub texture: Handle<Image>,
}

impl Material for SmokeMaterial {
    fn vertex_shader() -> ShaderRef {
        SMOKE_SHADER_HANDLE.into()
    }

    fn fragment_shader() -> ShaderRef {
        SMOKE_SHADER_HANDLE.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn specialize(
        _pipeline: &MaterialPipeline<Self>,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Built double-sided, like the other billboards.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// The ground overlay: an untextured fire mask whose colour comes entirely from
/// vertex colour, animated only where `sample_color` has marked a fragment as
/// glowing. See `shaders/fire_ground.wgsl`.
#[derive(Asset, TypePath, AsBindGroup, Clone, Default)]
pub struct FireGroundMaterial {
    /// Road distance field (`ground_mask`), so a scar can let the road show.
    #[texture(0)]
    #[sampler(1)]
    pub road_mask: Option<Handle<Image>>,
    #[uniform(2)]
    pub mask_rect: Vec4,
}

impl Material for FireGroundMaterial {
    fn fragment_shader() -> ShaderRef {
        GROUND_SHADER_HANDLE.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn specialize(
        _pipeline: &MaterialPipeline<Self>,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

const SCENE_LIGHT: Handle<Shader> =
    Handle::weak_from_u128(0x4B7E_119A_2CDD_44FF_9021_AABB_CCDD_EEF0);
pub struct FireShaderPlugin;

impl Plugin for FireShaderPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            FLAME_SHADER_HANDLE,
            "shaders/flame.wgsl",
            Shader::from_wgsl
        );
        load_internal_asset!(
            app,
            GROUND_SHADER_HANDLE,
            "shaders/fire_ground.wgsl",
            Shader::from_wgsl
        );
        load_internal_asset!(
            app,
            SMOKE_SHADER_HANDLE,
            "shaders/smoke.wgsl",
            Shader::from_wgsl
        );
        load_internal_asset!(
            app,
            SCENE_LIGHT,
            "shaders/scene_light.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins((
            MaterialPlugin::<FireMaterial>::default(),
            MaterialPlugin::<SmokeMaterial>::default(),
            MaterialPlugin::<FireGroundMaterial>::default(),
        ));
    }
}

//! The terrain's material (`shaders/terrain.wgsl`): the same on the desktop and
//! in the browser.

use bevy::asset::load_internal_asset;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderRef, ShaderType};

const SHADER_HANDLE: Handle<Shader> =
    Handle::weak_from_u128(0x91A7_44D2_6B0E_4C71_AA90_2201_88C4_2F10);

#[derive(Clone, Copy, Default, ShaderType)]
struct TerrainUniform {
    mask_rect: Vec4,
    ground_rect: Vec4,
    tactical: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Clone)]
pub struct TerrainMaterial {
    #[uniform(0)]
    uniform: TerrainUniform,
    /// Road distance field (`ground_mask`).
    #[texture(1)]
    #[sampler(2)]
    pub road_mask: Handle<Image>,
    /// Meadow cover, one texel per fire cell (`ground_mask::grass_map`).
    #[texture(3)]
    #[sampler(4)]
    pub grass_map: Handle<Image>,
    /// Ground colour (sRGB) and surface normal (Bevy frame), one texel per
    /// terrain sample, so the look does not depend on the mesh density.
    #[texture(5)]
    #[sampler(6)]
    pub ground_colour: Handle<Image>,
    #[texture(7)]
    #[sampler(8)]
    pub ground_normal: Handle<Image>,
    #[texture(9, sample_type = "float", filterable = false)]
    pub fire_state: Handle<Image>,
}

impl TerrainMaterial {
    /// `mask_rect`: world metres to mask coordinates
    /// (`ground_mask::RoadMask::rect`). `ground_rect`: world metres to the
    /// ground textures' uv: x, y = 1 / (posting * size), z = half a texel in
    /// u, w = the north edge plus half a texel in v.
    pub fn new(
        mask_rect: Vec4,
        ground_rect: Vec4,
        road_mask: Handle<Image>,
        grass_map: Handle<Image>,
        ground_colour: Handle<Image>,
        ground_normal: Handle<Image>,
        fire_state: Handle<Image>,
    ) -> Self {
        TerrainMaterial {
            uniform: TerrainUniform {
                mask_rect,
                ground_rect,
                tactical: Vec4::ZERO,
            },
            road_mask,
            grass_map,
            ground_colour,
            ground_normal,
            fire_state,
        }
    }

    pub fn set_time(&mut self, t: f32) {
        self.uniform.tactical.y = t;
    }

    /// How far the colours are drawn toward grey, 0..1 (`tactical.rs`).
    pub fn tactical(&self) -> f32 {
        self.uniform.tactical.x
    }

    pub fn set_tactical(&mut self, amount: f32) {
        self.uniform.tactical.x = amount;
    }
}

impl Material for TerrainMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER_HANDLE.into()
    }
}

pub(super) struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            SHADER_HANDLE,
            "../shaders/terrain.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default());
    }
}

//! The graphics level picked in the HUD, put on screen (see `quality`).
use bevy::core_pipeline::bloom::BloomSettings;
use bevy::core_pipeline::smaa::{SmaaPreset, SmaaSettings};
use bevy::pbr::{CascadeShadowConfig, CascadeShadowConfigBuilder, DirectionalLightShadowMap};
use bevy::prelude::*;

use crate::quality::{Quality, Rebuild};

/// Runs when the [`Quality`] resource changes: the sample count, the sun's
/// shadows, the post effects on the camera. What is baked into the scene (terrain
/// stride, plant density) is not applied here but by a rebuild, requested when
/// the knobs differ from those the scene was built with.
pub fn apply(
    quality: Res<Quality>,
    mut built: Local<Option<Quality>>,
    mut rebuild: ResMut<Rebuild>,
    mut commands: Commands,
    mut msaa: ResMut<Msaa>,
    mut shadow_map: ResMut<DirectionalLightShadowMap>,
    mut suns: Query<(&mut DirectionalLight, &mut CascadeShadowConfig), With<crate::sky::Sun>>,
    mut cameras: Query<(Entity, &mut BloomSettings, Has<SmaaSettings>), With<Camera3d>>,
) {
    // The scene was built with the level in force at the first run.
    let scene = *built.get_or_insert(*quality);
    if !quality.is_changed() {
        return;
    }
    if scene.needs_rebuild(&quality) {
        rebuild.0 = true;
        *built = Some(*quality);
    }
    if *msaa != quality.msaa_mode() {
        *msaa = quality.msaa_mode();
    }
    if shadow_map.size != quality.shadow_map {
        shadow_map.size = quality.shadow_map;
    }
    for (mut light, mut cascades) in &mut suns {
        light.shadows_enabled = quality.shadows;
        *cascades = CascadeShadowConfigBuilder {
            num_cascades: quality.cascades,
            minimum_distance: 0.1,
            maximum_distance: quality.shadow_distance,
            first_cascade_far_bound: 300.0,
            overlap_proportion: 0.2,
        }
        .build();
    }
    for (e, mut bloom, has_smaa) in &mut cameras {
        bloom.intensity = quality.bloom;
        match (quality.smaa && !cfg!(target_arch = "wasm32"), has_smaa) {
            (true, false) => {
                commands.entity(e).insert(SmaaSettings { preset: SmaaPreset::Medium });
            }
            (false, true) => {
                commands.entity(e).remove::<SmaaSettings>();
            }
            _ => {}
        }
    }
}

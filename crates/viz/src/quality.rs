//! Presentation-only presets. They never alter the simulation or operational overlays.
use bevy::prelude::*;
#[derive(Resource, Clone, Copy)]
pub struct Quality {
    pub name: &'static str,
    pub shadows: bool,
    pub bloom: f32,
    pub vegetation: f32,
    pub vegetation_distance: f32,
    pub fire_lights: usize,
}
impl Quality {
    pub fn from_env() -> Self {
        match std::env::var("TTX_QUALITY").as_deref() {
            Ok("low") => Self {
                name: "low",
                shadows: false,
                bloom: 0.0,
                vegetation: 0.35,
                vegetation_distance: 2500.0,
                fire_lights: 0,
            },
            Ok("high") => Self {
                name: "high",
                shadows: true,
                bloom: 0.18,
                vegetation: 1.0,
                vegetation_distance: 9000.0,
                fire_lights: 8,
            },
            _ if cfg!(target_arch = "wasm32") => Self {
                name: "low",
                shadows: false,
                bloom: 0.0,
                vegetation: 0.15,
                vegetation_distance: 1500.0,
                fire_lights: 0,
            },
            _ => Self {
                name: "projector",
                shadows: true,
                bloom: 0.06,
                vegetation: 0.65,
                vegetation_distance: 4500.0,
                fire_lights: 2,
            },
        }
    }
}

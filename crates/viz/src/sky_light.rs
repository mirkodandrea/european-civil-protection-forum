//! Image-based light from the procedural sky.
//!
//! The sky dome (`shaders/sky.wgsl`) is a fragment shader, so nothing in the
//! PBR pipeline can sample it. This module paints a CPU twin of its colours
//! into two small cube maps and hands them to the camera as an
//! [`EnvironmentMapLight`]: blue fill from above, warm ground bounce from
//! below and, on glossy paint, glass and metal, a reflection of the sky.
//!
//! * The *diffuse* map is the cosine-convolved sky, analytically: for a sky
//!   made of two hemispheres a normal sees `0.5 + 0.5 n.y` of the upper one.
//! * The *specular* map has a full mip chain. Mip `m` is the sky prefiltered
//!   for roughness `m / (mips - 1)` (Bevy picks the mip by perceptual
//!   roughness): the sharp radiance is blended toward the diffuse one and
//!   the sun's glow lobe widens. A brute-force GGX convolution is not needed
//!   for a smooth gradient and would cost far too much to redo in the
//!   browser.
//!
//! The maps are regenerated only when the sun has moved by a degree
//! ([`REGEN_DEG`]); the game's clock covers three hours around midday, so
//! that is a handful of times per game. Brightness (`intensity`) is updated
//! every frame because it is free.
//!
//! No compute shader, no storage texture: plain `Rgba16Float` cube images,
//! which WebGL2 accepts. The environment-map bindings are in the mesh view
//! layout whether or not a map is attached (a fallback cube fills them), so
//! this adds no texture units.
//!
//! Inert-safe: until the camera has the component, `SkyEnv::ambient_scale`
//! stays 1 and the flat ambient light is exactly what it was.

use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
    TextureViewDescriptor, TextureViewDimension,
};
use bevy::render::texture::{
    ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor,
};

use crate::sky::{sky_colors, SkyEnv, SunState};

/// Edge of the diffuse cube, texels.
const DIFFUSE_SIZE: u32 = 16;
/// Edge of the specular cube at mip 0, texels. Mips go down to 1x1.
const SPECULAR_SIZE: u32 = 64;
/// Regenerate when the sun elevation moved this much (degrees).
const REGEN_DEG: f32 = 1.0;
/// Fraction of the flat `AmbientLight` kept once the environment map is on.
/// The fill (this plus `ENV_GAIN`) sets how deep the shadows are: tuned with
/// the camera's colour grading (`main::grading`) on snapshots, 2026-10-20,
/// for shadows that read as shade, not as a paler copy of the sunlit side.
const AMBIENT_KEEP: f32 = 0.15;
/// `intensity` in units of the flat ambient brightness (whose texels are sky
/// radiances, about 0.4-0.8 by day).
const ENV_GAIN: f32 = 0.45;
/// Dry Mediterranean ground: albedo ~0.25 tinted ochre.
const GROUND_ALBEDO: [f32; 3] = [0.27, 0.22, 0.14];

pub struct SkyLightPlugin;

impl Plugin for SkyLightPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create_maps).add_systems(
            Update,
            (attach_to_camera, update_maps)
                .chain()
                .after(crate::sky::update_sky),
        );
    }
}

#[derive(Resource)]
struct SkyMaps {
    diffuse: Handle<Image>,
    specular: Handle<Image>,
}

/// Everything the radiance function reads, built from a sun elevation.
#[derive(Clone, Copy)]
struct SkyModel {
    zenith: Vec3,
    horizon: Vec3,
    sun_color: Vec3,
    sun_dir: Vec3,
    /// Upper-hemisphere mean, what a normal pointing up sees.
    up_avg: Vec3,
    ground: Vec3,
    /// 0..1 daylight gate for the sun's glow.
    day: f32,
}

impl SkyModel {
    fn new(el_deg: f32, sun_dir: Vec3, illuminance_ratio: f32) -> Self {
        let (z, h, s) = sky_colors(el_deg);
        let (zenith, horizon, sun_color) = (Vec3::from(z), Vec3::from(h), Vec3::from(s));
        let day = sun_dir.y.clamp(0.0, 1.0);
        let mut m = SkyModel {
            zenith,
            horizon,
            sun_color,
            sun_dir,
            up_avg: Vec3::ZERO,
            ground: Vec3::ZERO,
            day,
        };
        // Mean over the upper hemisphere of the gradient, cosine weighted
        // (numerically: a few elevation samples).
        let mut acc = Vec3::ZERO;
        let mut wsum = 0.0;
        for i in 0..8 {
            let y = (i as f32 + 0.5) / 8.0;
            let w = y; // cosine weight of a ring at elevation sin = y
            acc += m.gradient(y) * w;
            wsum += w;
        }
        m.up_avg = acc / wsum;
        // Bounce: sun on the ground plus the sky it reflects, times albedo.
        // `illuminance_ratio` is sun illuminance over the ambient brightness
        // (the texture's unit), divided by pi for radiance.
        let lit = sun_color * (illuminance_ratio / std::f32::consts::PI * day) + m.up_avg * 0.8;
        m.ground = Vec3::from(GROUND_ALBEDO) * lit;
        m
    }

    /// Dome gradient above the horizon (`sky.wgsl`).
    fn gradient(&self, y: f32) -> Vec3 {
        self.horizon.lerp(self.zenith, y.clamp(0.0, 1.0).powf(0.45))
    }

    /// Sharp radiance toward world direction `d`, without the sun.
    fn sharp(&self, d: Vec3) -> Vec3 {
        if d.y >= 0.0 {
            self.gradient(d.y)
        } else {
            self.horizon.lerp(self.ground, smoothstep(0.0, 0.18, -d.y))
        }
    }

    /// Cosine-convolved radiance for a surface normal `n`.
    fn diffuse(&self, n: Vec3) -> Vec3 {
        let w = (0.5 + 0.5 * n.y).clamp(0.0, 1.0);
        let w = smoothstep(0.0, 1.0, w);
        let base = self.ground.lerp(self.up_avg, w);
        // The bright band at the horizon leaks into sideways normals.
        let band = (1.0 - n.y * n.y).max(0.0);
        let sun = self.sun_color * (0.10 * self.day * n.dot(self.sun_dir).max(0.0).powi(2));
        base.lerp(self.horizon, 0.22 * band) + sun
    }

    /// Prefiltered radiance for roughness `r` in 0..1.
    fn specular(&self, d: Vec3, r: f32) -> Vec3 {
        // Sun glow: sharp lobe at r = 0, wide at r = 1. Amplitude falls with
        // the exponent so the lobe keeps roughly its energy.
        let e = 200.0 * (6.0f32 / 200.0).powf(r);
        let amp = 0.6 * (e / 200.0).sqrt();
        let lobe = self.sun_color * (self.day * amp * d.dot(self.sun_dir).max(0.0).powf(e));
        let k = r.clamp(0.0, 1.0).powf(0.7);
        (self.sharp(d) + lobe).lerp(self.diffuse(d), k)
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// World direction of a cube texel. Faces in wgpu order (+X -X +Y -Y +Z -Z);
/// Bevy samples with `(x, y, -z)`, so the world direction is the face
/// direction with z negated.
fn texel_dir(face: usize, u: f32, v: f32) -> Vec3 {
    let s = match face {
        0 => Vec3::new(1.0, -v, -u),
        1 => Vec3::new(-1.0, -v, u),
        2 => Vec3::new(u, 1.0, v),
        3 => Vec3::new(u, -1.0, -v),
        4 => Vec3::new(u, -v, 1.0),
        _ => Vec3::new(-u, -v, -1.0),
    };
    let s = s.normalize();
    Vec3::new(s.x, s.y, -s.z)
}

/// f32 -> IEEE half bits (round toward zero; values here are 0..~10).
fn f16_bits(x: f32) -> u16 {
    let x = if x.is_finite() {
        x.clamp(0.0, 60000.0)
    } else {
        0.0
    };
    let b = x.to_bits();
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let man = b & 0x007f_ffff;
    if exp <= 0 {
        if exp < -10 {
            return 0;
        }
        let m = (man | 0x0080_0000) >> (1 - exp);
        return (m >> 13) as u16;
    }
    if exp >= 31 {
        return 0x7bff;
    }
    ((exp as u16) << 10) | (man >> 13) as u16
}

/// One cube texture, layer-major (each face holds its whole mip chain).
fn bake(size: u32, mips: u32, f: impl Fn(Vec3, f32) -> Vec3) -> Vec<u8> {
    let mut data = Vec::new();
    for face in 0..6 {
        for m in 0..mips {
            let s = (size >> m).max(1);
            let r = if mips > 1 {
                m as f32 / (mips - 1) as f32
            } else {
                0.0
            };
            for y in 0..s {
                for x in 0..s {
                    let u = (x as f32 + 0.5) / s as f32 * 2.0 - 1.0;
                    let v = (y as f32 + 0.5) / s as f32 * 2.0 - 1.0;
                    let c = f(texel_dir(face, u, v), r);
                    for ch in [c.x, c.y, c.z, 1.0] {
                        data.extend_from_slice(&f16_bits(ch).to_le_bytes());
                    }
                }
            }
        }
    }
    data
}

fn mip_count(size: u32) -> u32 {
    32 - size.leading_zeros()
}

fn cube_image(size: u32, mips: u32, data: Vec<u8>) -> Image {
    Image {
        data,
        texture_descriptor: TextureDescriptor {
            label: Some("sky_light_cube"),
            size: Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 6,
            },
            mip_level_count: mips,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba16Float,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        sampler: ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::ClampToEdge,
            address_mode_v: ImageAddressMode::ClampToEdge,
            address_mode_w: ImageAddressMode::ClampToEdge,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            ..default()
        }),
        texture_view_descriptor: Some(TextureViewDescriptor {
            dimension: Some(TextureViewDimension::Cube),
            ..default()
        }),
        asset_usage: RenderAssetUsages::default(),
    }
}

fn images_for(model: &SkyModel) -> (Image, Image) {
    let sm = mip_count(SPECULAR_SIZE);
    let diffuse = cube_image(
        DIFFUSE_SIZE,
        1,
        bake(DIFFUSE_SIZE, 1, |d, _| model.diffuse(d)),
    );
    let specular = cube_image(
        SPECULAR_SIZE,
        sm,
        bake(SPECULAR_SIZE, sm, |d, r| model.specular(d, r)),
    );
    (diffuse, specular)
}

fn model_now(sky: &SkyEnv, sun: &SunState) -> SkyModel {
    let ratio = crate::sky::sun_illuminance(sky.el_deg) / sky.ambient_brightness.max(1.0);
    SkyModel::new(sky.el_deg, sun.direction, ratio)
}

fn create_maps(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    sky: Res<SkyEnv>,
    sun: Res<SunState>,
) {
    let (d, s) = images_for(&model_now(&sky, &sun));
    commands.insert_resource(SkyMaps {
        diffuse: images.add(d),
        specular: images.add(s),
    });
}

fn attach_to_camera(
    mut commands: Commands,
    maps: Option<Res<SkyMaps>>,
    mut sky: ResMut<SkyEnv>,
    cams: Query<Entity, (With<Camera3d>, Without<EnvironmentMapLight>)>,
) {
    let Some(maps) = maps else { return };
    for e in &cams {
        commands.entity(e).insert(EnvironmentMapLight {
            diffuse_map: maps.diffuse.clone(),
            specular_map: maps.specular.clone(),
            intensity: sky.ambient_brightness * ENV_GAIN,
        });
        sky.ambient_scale = AMBIENT_KEEP;
    }
}

fn update_maps(
    maps: Option<Res<SkyMaps>>,
    sky: Res<SkyEnv>,
    sun: Res<SunState>,
    mut images: ResMut<Assets<Image>>,
    mut cams: Query<&mut EnvironmentMapLight>,
    mut last_el: Local<Option<f32>>,
) {
    let Some(maps) = maps else { return };
    for mut env in &mut cams {
        env.intensity = sky.ambient_brightness * ENV_GAIN;
    }
    // The `SunState` direction is written in the same pass as `el_deg`; the
    // very first frame, before `update_sky` has run, still holds defaults.
    if let Some(prev) = *last_el {
        if (prev - sky.el_deg).abs() < REGEN_DEG {
            return;
        }
    }
    *last_el = Some(sky.el_deg);
    let (d, s) = images_for(&model_now(&sky, &sun));
    // Replace the data in place: same descriptor, same handle.
    if let Some(img) = images.get_mut(&maps.diffuse) {
        img.data = d.data;
    }
    if let Some(img) = images.get_mut(&maps.specular) {
        img.data = s.data;
    }
}

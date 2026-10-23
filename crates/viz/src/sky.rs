//! Sun, procedural sky and atmosphere from propagator_abm, with image-based
//! lighting. Presentation assumes 44.2°N and a representative summer day;
//! this is not scenario meteorology or a georeferenced solar ephemeris.
//! Scenario civil-clock hours drive the presentation as local solar hours.
//! TTX_START_HOUR optionally overrides the initial hour for visual review.
// ShaderType's generated `check` fns are never called.
#![allow(dead_code)]

use bevy::asset::load_internal_asset;
use bevy::pbr::{
    FogFalloff, FogSettings, Material, MaterialPipeline, MaterialPipelineKey, NotShadowCaster,
    NotShadowReceiver,
};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderRef, ShaderType, SpecializedMeshPipelineError,
};

use crate::{Scn, Snapshot};

/// A Ligurian latitude: the island is imaginary but Italian. Only latitude enters
/// the solar geometry below.
const SITE_LAT_DEG: f32 = 44.2;

/// A fixed representative midsummer day — Ligurian fire season peaks
/// July-August — used for the sun's declination. See the module doc for why
/// this is frozen rather than calendar-driven.
const DAY_OF_YEAR: f32 = 210.0;

const SKY_SHADER_HANDLE: Handle<Shader> =
    Handle::weak_from_u128(0x7A11_9C40_55EE_4B21_9E33_11AA_22BB_33CC);

/// Radius of the sky dome. Comfortably inside the camera's far plane
/// (40 000 m, see `main::setup_scene`) and comfortably outside anywhere the
/// orbit camera can reach around an 8 km scenario.
pub const SKY_RADIUS: f32 = 30_000.0;

/// Clouds travel faster than the surface wind the sea and the smoke read, and
/// a drift that is barely visible at 5 m/s would look like a still picture.
const CLOUD_SPEED_FACTOR: f32 = 5.0;

/// Fraction of the sky the clouds cover: scattered, so the sun still shows.
const CLOUD_COVERAGE: f32 = 0.5;

/// Local solar time at `T+0`, in hours.
#[derive(Resource)]
pub struct DayClock {
    pub start_hour: f32,
}

impl DayClock {
    /// Hour of day `update_sky` should light the scene for.
    pub fn hour(&self, sim_time_s: f32) -> f32 {
        self.start_hour + sim_time_s / 3600.0
    }
}

impl Default for DayClock {
    fn default() -> Self {
        let start_hour = std::env::var("TTX_START_HOUR")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(13.0);
        DayClock {
            start_hour: start_hour.rem_euclid(24.0),
        }
    }
}

/// The sun's direction and colour as of the last [`update_sky`] run, shared
/// with anything else that needs to agree with it — currently `crate::sea`.
#[derive(Resource, Clone, Copy)]
#[allow(dead_code)]
pub struct SunState {
    /// Unit vector from the ground toward the sun, in Bevy world space.
    pub direction: Vec3,
    pub color: Vec3,
    /// 0 at and under the horizon, 1 at full daylight. A `sin(elevation)`
    /// gate — deliberately conservative, so it is right for gating things
    /// that should have no business happening at night (a sun glint) but
    /// wrong for anything meant to track how bright the *scene* actually is:
    /// it is still only ~0.5 at a properly sunny 30° mid-afternoon sun.
    pub day_gate: f32,
    /// `illuminance / ILLUMINANCE`'s own daylight ceiling, clamped to
    /// [0, 1] — tracks the same curve the `DirectionalLight` itself is lit
    /// from, so anything blended by this looks as bright as the rest of the
    /// sunlit scene rather than dimming early the way `day_gate` does.
    pub brightness: f32,
}

impl Default for SunState {
    fn default() -> Self {
        SunState {
            direction: Vec3::Y,
            color: Vec3::ONE,
            day_gate: 0.0,
            brightness: 0.0,
        }
    }
}

/// Marks the single `DirectionalLight` entity [`update_sky`] drives. Spawned
/// in `main::setup_scene` with no particular transform or illuminance — both
/// are overwritten the first time `update_sky` runs.
#[derive(Component)]
pub struct Sun;

#[derive(Component)]
pub struct SkyDome;

#[derive(Resource)]
pub(crate) struct SkyHandle(pub(crate) Handle<SkyMaterial>);

#[derive(Clone, Copy, Default, ShaderType)]
pub struct SkyUniform {
    pub sun_dir: Vec4,
    pub sun_color: Vec4,
    pub zenith_color: Vec4,
    pub horizon_color: Vec4,
    pub moon_dir: Vec4,
    pub moon_color: Vec4,
    /// xy: how far the cloud layer has been carried downwind, metres (Bevy XZ).
    /// z: coverage, 0..1.
    pub cloud: Vec4,
    /// rgb: sunlit cloud tops; a: unused.
    pub cloud_lit: Vec4,
    /// rgb: cloud undersides.
    pub cloud_shade: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct SkyMaterial {
    #[uniform(0)]
    pub uniform: SkyUniform,
}

impl Material for SkyMaterial {
    fn fragment_shader() -> ShaderRef {
        SKY_SHADER_HANDLE.into()
    }

    fn specialize(
        _pipeline: &MaterialPipeline<Self>,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Seen from inside the dome; winding is whatever `uv_sphere` produced
        // and does not matter here.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// What the image-based light (`crate::sky_light`) needs from the sky, written
/// by [`update_sky`] every frame.
#[derive(Resource, Clone, Copy)]
pub struct SkyEnv {
    pub el_deg: f32,
    /// The flat ambient brightness before `ambient_scale` is applied: the
    /// unit the environment map's `intensity` is expressed in.
    pub ambient_brightness: f32,
    /// Fraction of the flat `AmbientLight` kept. 1 until the environment
    /// map is attached to a camera, then lowered, so a failure to set the
    /// map up leaves the old look.
    pub ambient_scale: f32,
}

impl Default for SkyEnv {
    fn default() -> Self {
        SkyEnv {
            el_deg: 45.0,
            ambient_brightness: 1000.0,
            ambient_scale: 1.0,
        }
    }
}

/// Zenith, horizon and sun colours for a sun elevation (degrees).
pub fn sky_colors(el_deg: f32) -> ([f32; 3], [f32; 3], [f32; 3]) {
    (
        ramp3(el_deg, &ZENITH),
        ramp3(el_deg, &HORIZON),
        ramp3(el_deg, &SUN_COLOR),
    )
}

/// Sun illuminance (lux) for an elevation.
pub fn sun_illuminance(el_deg: f32) -> f32 {
    ramp1(el_deg, &ILLUMINANCE)
}

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            SKY_SHADER_HANDLE,
            "shaders/sky.wgsl",
            Shader::from_wgsl
        );
        app.init_resource::<DayClock>()
            .init_resource::<SkyEnv>()
            .add_plugins(crate::sky_light::SkyLightPlugin)
            .init_resource::<SunState>()
            .add_plugins(MaterialPlugin::<SkyMaterial>::default())
            .add_systems(Startup, spawn_sky)
            .add_systems(Update, update_sky.after(crate::sync));
    }
}

fn spawn_sky(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
) {
    let mesh = meshes.add(uv_sphere(SKY_RADIUS, 24, 16));
    let handle = materials.add(SkyMaterial {
        uniform: SkyUniform::default(),
    });
    commands.insert_resource(SkyHandle(handle.clone()));
    commands.spawn((
        MaterialMeshBundle {
            mesh,
            material: handle,
            ..default()
        },
        SkyDome,
        // A 30 km sphere left as a default shadow caster/receiver blows up
        // the directional light's cascade-fitting: Bevy sizes each cascade's
        // shadow-map depth range to cover every caster inside that cascade's
        // view slice, and the dome's radius dwarfs everything else in the
        // scene by two to three orders of magnitude. The near cascade's
        // depth range collapses trying to span metres to tens of
        // kilometres in one map, and the failure reads as a solid,
        // roughly-view-aligned shadow that tracks the camera rather than
        // the terrain — because the dome itself is always centred on
        // wherever the camera happens to be standing.
        NotShadowCaster,
        NotShadowReceiver,
    ));
}

/// A plain UV sphere. Vertex count barely matters here — the dome is shaded
/// entirely from the fragment shader's view direction, not from baked vertex
/// data — so a coarse sphere is exactly as smooth as a fine one.
fn uv_sphere(radius: f32, sectors: usize, stacks: usize) -> Mesh {
    let mut positions = Vec::with_capacity((sectors + 1) * (stacks + 1));
    let mut normals = Vec::with_capacity(positions.capacity());
    let mut uvs = Vec::with_capacity(positions.capacity());

    for i in 0..=stacks {
        let phi = std::f32::consts::PI * i as f32 / stacks as f32;
        let (sin_phi, cos_phi) = phi.sin_cos();
        for j in 0..=sectors {
            let theta = std::f32::consts::TAU * j as f32 / sectors as f32;
            let (sin_theta, cos_theta) = theta.sin_cos();
            let (x, y, z) = (sin_phi * cos_theta, cos_phi, sin_phi * sin_theta);
            positions.push([x * radius, y * radius, z * radius]);
            normals.push([x, y, z]);
            uvs.push([j as f32 / sectors as f32, i as f32 / stacks as f32]);
        }
    }

    let row = sectors + 1;
    let mut indices = Vec::with_capacity(stacks * sectors * 6);
    for i in 0..stacks {
        for j in 0..sectors {
            let a = (i * row + j) as u32;
            let b = a + row as u32;
            indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Simplified solar position: elevation and azimuth (radians, azimuth
/// clockwise from north) for a given local solar hour. Standard declination
/// / hour-angle formula; see the module doc for what it deliberately skips.
fn solar_position(hour_of_day: f32) -> (f32, f32) {
    let lat = SITE_LAT_DEG.to_radians();
    let decl =
        23.44f32.to_radians() * (((360.0 / 365.0) * (DAY_OF_YEAR + 284.0)).to_radians()).sin();
    let h = (hour_of_day - 12.0) * 15f32.to_radians();

    let el = (lat.sin() * decl.sin() + lat.cos() * decl.cos() * h.cos())
        .clamp(-1.0, 1.0)
        .asin();
    let az_cos = ((decl.sin() - el.sin() * lat.sin()) / (el.cos() * lat.cos())).clamp(-1.0, 1.0);
    let az = if h.rem_euclid(std::f32::consts::TAU) <= std::f32::consts::PI {
        az_cos.acos()
    } else {
        std::f32::consts::TAU - az_cos.acos()
    };
    (el, az)
}

/// Unit vector from the ground toward the sun, in Bevy world space (+x east,
/// +y up, +z south — see `crate::frame`).
fn sun_direction(elevation: f32, azimuth: f32) -> Vec3 {
    let horiz = elevation.cos();
    Vec3::new(
        azimuth.sin() * horiz,
        elevation.sin(),
        -azimuth.cos() * horiz,
    )
}

/// Piecewise-linear interpolation over sorted `(elevation_deg, value)` stops,
/// clamped past the ends. Same idea as `fire_view::ramp`, kept local because
/// the stop tables here are colours *and* scalars.
fn ramp3(t: f32, stops: &[(f32, [f32; 3])]) -> [f32; 3] {
    let t = t.clamp(stops[0].0, stops[stops.len() - 1].0);
    for pair in stops.windows(2) {
        let (t0, c0) = pair[0];
        let (t1, c1) = pair[1];
        if t <= t1 {
            let k = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
            return [
                c0[0] + (c1[0] - c0[0]) * k,
                c0[1] + (c1[1] - c0[1]) * k,
                c0[2] + (c1[2] - c0[2]) * k,
            ];
        }
    }
    stops[stops.len() - 1].1
}

fn ramp1(t: f32, stops: &[(f32, f32)]) -> f32 {
    let t = t.clamp(stops[0].0, stops[stops.len() - 1].0);
    for pair in stops.windows(2) {
        let (t0, v0) = pair[0];
        let (t1, v1) = pair[1];
        if t <= t1 {
            let k = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
            return v0 + (v1 - v0) * k;
        }
    }
    stops[stops.len() - 1].1
}

const ILLUMINANCE: [(f32, f32); 6] = [
    (-90.0, 0.0),
    (-6.0, 15.0),
    (0.0, 600.0),
    (10.0, 4200.0),
    (30.0, 8200.0),
    (60.0, 10_500.0),
];
const SUN_COLOR: [(f32, [f32; 3]); 6] = [
    (-90.0, [0.60, 0.60, 0.70]),
    (-6.0, [1.00, 0.35, 0.18]),
    (0.0, [1.00, 0.50, 0.25]),
    (10.0, [1.00, 0.75, 0.52]),
    (30.0, [1.00, 0.90, 0.78]),
    (60.0, [1.00, 0.96, 0.90]),
];
const ZENITH: [(f32, [f32; 3]); 6] = [
    (-90.0, [0.02, 0.03, 0.07]),
    (-6.0, [0.05, 0.07, 0.16]),
    (0.0, [0.10, 0.14, 0.28]),
    (10.0, [0.16, 0.28, 0.52]),
    (30.0, [0.20, 0.38, 0.68]),
    (60.0, [0.22, 0.42, 0.75]),
];
const HORIZON: [(f32, [f32; 3]); 6] = [
    (-90.0, [0.03, 0.04, 0.09]),
    (-6.0, [0.30, 0.18, 0.20]),
    (0.0, [0.85, 0.45, 0.28]),
    (10.0, [0.78, 0.62, 0.48]),
    (30.0, [0.68, 0.74, 0.80]),
    (60.0, [0.62, 0.73, 0.84]),
];
/// Open shade under a clear sky runs at roughly a sixth to an eighth of
/// direct sun, not the couple of orders of magnitude a hand-tuned, much
/// smaller ambient constant (this shipped at a flat 130 against a 9 500 lux
/// sun — a 73:1 ratio) implies. That mismatch is what a self-shadowed
/// hillside close to the camera exposed: correct terrain self-shadowing,
/// crushed to flat black because the ambient fill light was never scaled to
/// the direct sun it now has to counter across a full day. Tied to
/// `illuminance` rather than its own elevation ramp so the two can never
/// drift back out of proportion.
const AMBIENT_FILL_RATIO: f32 = 6.0;
const AMBIENT_COLOR: [(f32, [f32; 3]); 6] = [
    (-90.0, [0.05, 0.07, 0.14]),
    (-6.0, [0.20, 0.16, 0.22]),
    (0.0, [0.45, 0.35, 0.35]),
    (10.0, [0.55, 0.55, 0.55]),
    (30.0, [0.68, 0.74, 0.85]),
    (60.0, [0.72, 0.78, 0.92]),
];

/// The moon is always full: this scenario is never open long enough (1-3 h)
/// for a phase to matter, and a full moon is the one phase that is actually
/// opposite the sun in the sky — which is also what makes `-dir` the right
/// direction for it, no separate ephemeris needed. `moon_direction` and
/// `moon_direction_elevation` below are exactly that mirroring, spelled out
/// so the "why -dir" isn't left implicit at the call site.
///
/// Colour warms near the horizon the same way the sun's does (Rayleigh
/// scattering through more atmosphere), fading to a pale cool white
/// overhead — a full moon has none of the sun's saturated orange, since it is
/// reflected light with none of the sun's own colour temperature swing.
const MOON_COLOR: [(f32, [f32; 3]); 3] = [
    (0.0, [0.80, 0.62, 0.48]),
    (20.0, [0.85, 0.87, 0.93]),
    (90.0, [0.82, 0.87, 1.00]),
];

/// How much a full moon actually brightens a clear night — a few tens of lux
/// at most, nothing like the sun's thousands, but well above zero: a full
/// moon overhead casts shadows a new moon night does not. Zero below the
/// horizon by construction (the first two stops), so this needs no separate
/// gate.
const MOON_ILLUMINANCE: [(f32, f32); 4] = [(-90.0, 0.0), (0.0, 0.0), (30.0, 40.0), (90.0, 60.0)];

/// The ambient tint a risen full moon adds, blended into `AMBIENT_COLOR`'s
/// night entries in proportion to how high the moon is — cooler and a touch
/// brighter than the flat navy floor, because that floor was tuned as "no
/// light source at all" and a full moon is very much a light source.
const MOON_AMBIENT_TINT: [(f32, [f32; 3]); 3] = [
    (0.0, [0.10, 0.13, 0.22]),
    (30.0, [0.16, 0.20, 0.34]),
    (90.0, [0.20, 0.25, 0.40]),
];

pub fn update_sky(
    sim: Res<Snapshot>,
    scn: Res<Scn>,
    day: Res<DayClock>,
    sky_handle: Option<Res<SkyHandle>>,
    mut sky_materials: ResMut<Assets<SkyMaterial>>,
    mut sun_state: ResMut<SunState>,
    mut ambient: ResMut<AmbientLight>,
    mut sky_env: ResMut<SkyEnv>,
    mut clear_color: ResMut<ClearColor>,
    mut sun_q: Query<(&mut Transform, &mut DirectionalLight), With<Sun>>,
    mut fog_q: Query<&mut FogSettings>,
    mut dome_q: Query<(&mut Visibility, &mut Transform), (With<SkyDome>, Without<Sun>)>,
    cams: Query<&Transform, (With<Camera3d>, Without<Sun>, Without<SkyDome>)>,
    time: Res<Time>,
    mut cloud_offset: Local<Vec2>,
) {
    // The layer is carried by the wind the sea reads, and integrated, so a
    // wind shift turns the drift without making the clouds jump. Real time,
    // not sim time: at ×0 the sky stays alive, at ×N it does not race.
    let weather = sim.weather.unwrap_or(scn.0.weather);
    let a = (weather.wind_dir_deg as f32 + 180.0).to_radians();
    let drift = Vec2::new(a.sin(), -a.cos());
    let wind = weather.wind_speed_kmh as f32 / 3.6;
    *cloud_offset += drift * wind * CLOUD_SPEED_FACTOR * time.delta_seconds();

    let hour = if std::env::var("TTX_START_HOUR").is_ok() {
        day.hour(sim.time_s as f32)
    } else {
        scn.0.start_clock_s.rem_euclid(86400) as f32 / 3600.0 + sim.time_s as f32 / 3600.0
    };
    let (el, az) = solar_position(hour);
    let el_deg = el.to_degrees();
    let dir = sun_direction(el, az);

    let illuminance = ramp1(el_deg, &ILLUMINANCE);
    let sun_color = ramp3(el_deg, &SUN_COLOR);
    let zenith = ramp3(el_deg, &ZENITH);
    let horizon = ramp3(el_deg, &HORIZON);
    let day_gate = el.sin().clamp(0.0, 1.0);
    let brightness = (illuminance / ILLUMINANCE[ILLUMINANCE.len() - 1].1).clamp(0.0, 1.0);

    // The moon sits exactly opposite the sun (see `MOON_COLOR`'s doc), so its
    // elevation is the sun's negated and its direction is simply `-dir`.
    let moon_el_deg = -el_deg;
    let moon_gate = (-el).sin().clamp(0.0, 1.0);
    let moon_dir = -dir;
    let moon_color = ramp3(moon_el_deg, &MOON_COLOR);
    let moon_illuminance = ramp1(moon_el_deg, &MOON_ILLUMINANCE);

    // A floor so night keeps a faint moonlight-like fill rather than going to
    // literal zero, same spirit as the sun's own illuminance floor — plus the
    // *actual* moon's contribution on top, so a full moon overhead reads as
    // brighter than one that has just set.
    let ambient_brightness = (illuminance / AMBIENT_FILL_RATIO).max(4.0) + moon_illuminance;
    let base_ambient_color = ramp3(el_deg, &AMBIENT_COLOR);
    let moon_tint = ramp3(moon_el_deg, &MOON_AMBIENT_TINT);
    let ambient_color = [
        base_ambient_color[0] + (moon_tint[0] - base_ambient_color[0]) * moon_gate,
        base_ambient_color[1] + (moon_tint[1] - base_ambient_color[1]) * moon_gate,
        base_ambient_color[2] + (moon_tint[2] - base_ambient_color[2]) * moon_gate,
    ];

    *sun_state = SunState {
        direction: dir,
        color: Vec3::from(sun_color),
        day_gate,
        brightness,
    };

    for (mut transform, mut light) in &mut sun_q {
        *transform = Transform::IDENTITY.looking_to(-dir, Vec3::Y);
        light.illuminance = illuminance;
        light.color = Color::srgb(sun_color[0], sun_color[1], sun_color[2]);
    }

    sky_env.el_deg = el_deg;
    sky_env.ambient_brightness = ambient_brightness;
    ambient.brightness = ambient_brightness * sky_env.ambient_scale;
    ambient.color = Color::srgb(ambient_color[0], ambient_color[1], ambient_color[2]);
    clear_color.0 = Color::srgb(horizon[0], horizon[1], horizon[2]);

    if let Some(handle) = sky_handle {
        if let Some(mat) = sky_materials.get_mut(&handle.0) {
            mat.uniform = SkyUniform {
                sun_dir: dir.extend(day_gate),
                sun_color: Vec3::from(sun_color).extend(1.0),
                zenith_color: Vec3::from(zenith).extend(1.0),
                horizon_color: Vec3::from(horizon).extend(1.0),
                moon_dir: moon_dir.extend(moon_gate),
                moon_color: Vec3::from(moon_color).extend(1.0),
                cloud: Vec4::new(cloud_offset.x, cloud_offset.y, CLOUD_COVERAGE, 0.0),
                cloud_lit: (Vec3::from(sun_color) * (0.12 + 0.88 * brightness)
                    + Vec3::splat(0.05 * moon_gate))
                .extend(1.0),
                cloud_shade: (Vec3::from(horizon) * 0.55 * (0.2 + 0.8 * brightness)
                    + Vec3::splat(0.03 * moon_gate))
                .extend(1.0),
            };
        }
    }

    for mut fog in &mut fog_q {
        let horizon_col = Color::srgb(horizon[0], horizon[1], horizon[2]);
        fog.color = horizon_col;
        fog.directional_light_color = Color::srgba(sun_color[0], sun_color[1], sun_color[2], 0.5);
        fog.directional_light_exponent = 30.0;
        // Generous visibility: the incident area is 10 km across and the
        // point of the haze is depth cues on the far ridgeline, not a wall.
        fog.falloff = FogFalloff::from_visibility_colors(
            11_000.0,
            horizon_col,
            Color::srgb(horizon[0] * 0.92, horizon[1] * 0.94, horizon[2] * 0.98),
        );
    }

    for (mut vis, mut tf) in &mut dome_q {
        *vis = Visibility::Inherited;
        if let Ok(cam) = cams.get_single() {
            tf.translation = cam.translation;
        }
    }
}

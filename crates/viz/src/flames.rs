//! Flames, smoke and sparks, driven by the Monte Carlo fire.
//!
//! Builds on `propagator_abm`'s approach (Byram flame-length billboards with a
//! domain-warped shader, a flaming band behind the front, smoke particles) and
//! adds what the ensemble makes possible:
//!
//! - **Intensity → flame length.** Each tongue's height is Byram's flame length
//!   `L = 0.0775·I^0.46` of the cell's *ensemble-mean* fireline intensity (mean
//!   over the runs that reached it), times one constant visual exaggeration
//!   ([`FLAME_EXAGGERATION`]) so a 4 m flame reads from 2 km. Relative
//!   heights stay physical.
//! - **Probability → confidence.** In the actual fire (cells reached by ≥ cutoff
//!   of the runs) the height and the number of tongues scale with the fraction
//!   of runs that burned the cell. In the uncertainty halo (reached by only
//!   some runs) short, dim, redder tongues show where the fire *might* be,
//!   with brightness proportional to probability.
//! - **Wind tilt.** Tongues lean downwind by `θ = atan(U / √(g·L))` (a Froude-number
//!   tilt, capped at 65°), so a wind-driven head fire lies over while a flank
//!   stands up.
//! - **Life cycle.** A cell flames for [`FLAMING_S`] after its (quantile)
//!   arrival time, strongest in the first minutes; the ground then glows and
//!   cools to a scar (see `terrain.rs`) and smokes.
//!
//! Geometry is rebuilt every frame (a few thousand quads at most), so the fire
//! stays alive between simulation updates and at any speed.

use bevy::asset::load_internal_asset;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderRef, SpecializedMeshPipelineError, TextureDimension, TextureFormat};
use scenario::{Cell, Pos};

use crate::frame::{height_at, to_bevy, EXAGGERATION};
use crate::{Scn, Snapshot};

const FLAME_SHADER: Handle<Shader> = Handle::weak_from_u128(0x6E0B_5C2A_91F4_4D3B_8A77_1C2D_3E4F_5061);

/// Visual exaggeration of flame height (real flames here are 1–6 m).
pub const FLAME_EXAGGERATION: f32 = 9.0;
/// Seconds a cell flames after the front arrives (the rest of its 20-minute
/// residence time is glowing and smouldering).
pub const FLAMING_S: f32 = 420.0;
/// Seconds a burnt-out cell keeps smoking.
const SMOKING_S: f32 = 1800.0;
const MAX_TONGUES: usize = 4000;
const MAX_SMOKE: usize = 900;
const MAX_SPARKS: usize = 500;
const G: f32 = 9.81;

pub struct FlamesPlugin;

impl Plugin for FlamesPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, FLAME_SHADER, "shaders/flame.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<FlameMaterial>::default())
            .add_systems(Startup, setup)
            .add_systems(Update, update.after(crate::frame::update));
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct FlameMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub texture: Handle<Image>,
}

impl Material for FlameMaterial {
    fn fragment_shader() -> ShaderRef {
        FLAME_SHADER.into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
    fn specialize(_: &MaterialPipeline<Self>, d: &mut RenderPipelineDescriptor, _: &MeshVertexBufferLayoutRef, _: MaterialPipelineKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        d.primitive.cull_mode = None;
        Ok(())
    }
}

struct Puff {
    pos: Vec3,
    vel: Vec3,
    age: f32,
    life: f32,
    size: f32,
    shade: f32,
}

struct Spark {
    pos: Vec3,
    vel: Vec3,
    age: f32,
    life: f32,
}

#[derive(Resource)]
pub struct FlameView {
    flames: Handle<Mesh>,
    smoke: Handle<Mesh>,
    sparks: Handle<Mesh>,
    puffs: Vec<Puff>,
    embers: Vec<Spark>,
    rng: u64,
}

impl FlameView {
    fn rand(&mut self) -> f32 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        ((self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
    }
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut images: ResMut<Assets<Image>>, mut flame_mats: ResMut<Assets<FlameMaterial>>, mut mats: ResMut<Assets<StandardMaterial>>) {
    let flames = meshes.add(empty_mesh());
    let smoke = meshes.add(empty_mesh());
    let sparks = meshes.add(empty_mesh());
    let tongue = images.add(tongue_texture());
    let puff = images.add(puff_texture());
    commands.spawn(MaterialMeshBundle { mesh: flames.clone(), material: flame_mats.add(FlameMaterial { texture: tongue }), ..default() });
    commands.spawn(PbrBundle {
        mesh: smoke.clone(),
        material: mats.add(StandardMaterial { base_color_texture: Some(puff.clone()), alpha_mode: AlphaMode::Blend, unlit: true, cull_mode: None, double_sided: true, ..default() }),
        ..default()
    });
    commands.spawn(PbrBundle {
        mesh: sparks.clone(),
        material: mats.add(StandardMaterial { base_color_texture: Some(puff), alpha_mode: AlphaMode::Add, unlit: true, cull_mode: None, ..default() }),
        ..default()
    });
    commands.insert_resource(FlameView { flames, smoke, sparks, puffs: Vec::new(), embers: Vec::new(), rng: 0x9E37_79B9_7F4A_7C15 });
}

fn empty_mesh() -> Mesh {
    QuadBuilder::default().finish()
}

#[derive(Default)]
struct QuadBuilder {
    p: Vec<[f32; 3]>,
    n: Vec<[f32; 3]>,
    c: Vec<[f32; 4]>,
    uv: Vec<[f32; 2]>,
    ix: Vec<u32>,
}

impl QuadBuilder {
    /// A quad from its four corners (bottom-left, bottom-right, top-right, top-left).
    fn quad(&mut self, corners: [Vec3; 4], bottom: [f32; 4], top: [f32; 4]) {
        let b = self.p.len() as u32;
        for (k, v) in corners.iter().enumerate() {
            self.p.push(v.to_array());
            self.n.push([0.0, 1.0, 0.0]);
            self.c.push(if k < 2 { bottom } else { top });
        }
        self.uv.extend_from_slice(&[[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
        self.ix.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }

    fn finish(self) -> Mesh {
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.p);
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.n);
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.c);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uv);
        m.insert_indices(Indices::U32(self.ix));
        m
    }
}

fn hash01(x: u64) -> f32 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 24) as f32
}

/// Flame tilt from vertical, radians: `atan(U / √(g·L))`, capped at 65°.
pub fn tilt_angle(wind_ms: f32, flame_m: f32) -> f32 {
    if flame_m <= 0.0 {
        return 0.0;
    }
    (wind_ms / (G * flame_m).sqrt()).atan().min(65f32.to_radians())
}

/// The tilt as drawn: 70 % of the physical angle, at most 50°. A 60°-tilted
/// tongue seen from above is a streak on the ground; this keeps the lean
/// readable while strong winds still visibly lay the head fire over.
pub fn drawn_tilt(wind_ms: f32, flame_m: f32) -> f32 {
    (tilt_angle(wind_ms, flame_m) * 0.7).min(50f32.to_radians())
}

/// One tongue's geometry and colour (pure, so it can be tested).
pub struct Tongue {
    pub height: f32,
    pub half_width: f32,
    pub heat: f32,
    pub alpha: f32,
}

/// Tongue parameters for a cell: `fli` kW/m, `p` probability, `flaming` in
/// 0..1 (1 = the front just arrived), `halo` = below the cutoff.
pub fn tongue_for(fli: f32, p: f32, flaming: f32, halo: bool, cutoff: f32) -> Tongue {
    let l = fire::flame_length_m(fli).max(1.2);
    if halo {
        let conf = (p / cutoff.max(1e-3)).clamp(0.0, 1.0);
        let height = l * FLAME_EXAGGERATION * 0.35 * (0.4 + 0.6 * conf);
        return Tongue { height, half_width: (height * 0.34).min(12.0), heat: 0.55, alpha: 0.25 + 0.45 * conf };
    }
    let height = l * FLAME_EXAGGERATION * (0.45 + 0.55 * flaming) * (0.55 + 0.45 * p);
    let heat = (fli / 3000.0).clamp(0.6, 2.2) * (0.55 + 0.45 * flaming);
    Tongue { height, half_width: (height * 0.34).min(14.0), heat, alpha: 1.0 }
}

#[allow(clippy::too_many_arguments)]
fn update(
    snap: Res<Snapshot>,
    scn: Res<Scn>,
    view: Option<ResMut<FlameView>>,
    mut meshes: ResMut<Assets<Mesh>>,
    cam: Query<&Transform, With<Camera3d>>,
    time: Res<Time>,
) {
    let (Some(mut view), Ok(cam)) = (view, cam.get_single()) else { return };
    let s = &scn.0;
    let n = s.grid.len();
    if snap.cells.len() != n || snap.intensity.len() != n {
        return;
    }
    let t = time.elapsed_seconds();
    let dt = time.delta_seconds().min(0.1);
    let now = snap.time_s as f32;
    let cutoff = snap.ensemble.map(|e| e.cutoff).unwrap_or(0.5);
    let w = snap.weather.unwrap_or(scenario::WeatherSpec { wind_dir_deg: 0.0, wind_speed_kmh: 0.0, moisture_pct: 5.0 });
    let wind_ms = w.wind_speed_kmh as f32 / 3.6;
    // Wind blows *from* wind_dir; downwind unit vector in the world frame (x east, y north).
    let to = (w.wind_dir_deg as f32 + 180.0).to_radians();
    let down_world = Vec2::new(to.sin(), to.cos());
    let down = Vec3::new(down_world.x, 0.0, -down_world.y);
    // Upright billboards facing the camera horizontally.
    let mut right = cam.rotation * Vec3::X;
    right.y = 0.0;
    let right = right.normalize_or_zero();

    let mut flames = QuadBuilder::default();
    let mut sources: Vec<(Vec3, f32)> = Vec::new();
    let mut tongues = 0usize;
    let cols = s.grid.cols;
    let half_cell = s.grid.cellsize * 0.5;

    for i in 0..n {
        let code = snap.cells[i];
        let p = snap.probability[i];
        let halo = code == 5;
        let burning = code == 1;
        if !(burning || (halo && p >= 0.08)) {
            if code == 2 && snap.arrival[i] != i32::MIN && now - (snap.arrival[i] as f32) < SMOKING_S {
                // Recently burnt: smoulder, feeding the plume a little.
                if hash01(i as u64 * 31 + (t * 2.0) as u64) < 0.004 {
                    let c = s.cell_centre(Cell { row: i / cols, col: i % cols });
                    sources.push((to_bevy(s, c, 2.0), 0.25));
                }
            }
            continue;
        }
        let fli = snap.intensity[i];
        let flaming = if burning && snap.arrival[i] != i32::MIN { (1.0 - (now - snap.arrival[i] as f32) / FLAMING_S).clamp(0.0, 1.0) } else { 0.6 };
        if burning && flaming < 0.03 {
            // Past the flaming band: glowing ground (terrain.rs) and smoke.
            if hash01(i as u64 * 17 + (t * 3.0) as u64) < 0.01 {
                let c = s.cell_centre(Cell { row: i / cols, col: i % cols });
                sources.push((to_bevy(s, c, 2.0), 0.5));
            }
            continue;
        }
        let base = tongue_for(fli, p, flaming, halo, cutoff);
        let count = if halo { 1 } else { (1.0 + 3.0 * p * (fli / 3000.0).min(1.5) * flaming).round() as usize };
        let centre = s.cell_centre(Cell { row: i / cols, col: i % cols });
        for k in 0..count {
            if tongues >= MAX_TONGUES {
                break;
            }
            tongues += 1;
            let h1 = hash01(i as u64 * 7919 + k as u64 * 31);
            let h2 = hash01(i as u64 * 104_729 + k as u64 * 97);
            let pos: Pos = [centre[0] + (h1 - 0.5) * 2.0 * half_cell, centre[1] + (h2 - 0.5) * 2.0 * half_cell];
            let ground = Vec3::new(pos[0] - s.grid.width_m() * 0.5, height_at(s, pos) * EXAGGERATION - 0.5, -(pos[1] - s.grid.height_m() * 0.5));
            let phase = h1 * 40.0 + k as f32 * 2.1;
            let flicker = 0.7 + 0.3 * (t * 5.3 + phase).sin() * (t * 2.1 + phase * 0.7).cos();
            let h = base.height * flicker * (0.75 + 0.5 * h2);
            let hw = base.half_width * (0.8 + 0.4 * h1);
            // Froude tilt on the real flame length, applied to the drawn height.
            let theta = drawn_tilt(wind_ms, h / FLAME_EXAGGERATION);
            let lean = down * (theta.tan() * h) + right * ((t * 2.7 + phase).sin() * h * 0.06);
            let top_centre = ground + Vec3::Y * h + lean;
            let taper = 0.45;
            let heat = base.heat;
            let a = base.alpha;
            let (bottom, top) = if halo {
                ([2.2 * heat, 0.55 * heat, 0.12 * heat, a], [1.3 * heat, 0.22 * heat, 0.05 * heat, a])
            } else {
                ([3.4 * heat, 1.5 * heat, 0.3 * heat, a], [2.1 * heat, 0.55 * heat, 0.08 * heat, a])
            };
            flames.quad([ground - right * hw, ground + right * hw, top_centre + right * hw * taper, top_centre - right * hw * taper], bottom, top);
            if !halo && k == 0 {
                sources.push((ground + Vec3::Y * h * 0.8 + lean * 0.8, (fli / 3000.0).clamp(0.3, 2.0) * flaming.max(0.3)));
            }
        }
    }
    meshes.insert(&view.flames, flames.finish());

    // ---- smoke and sparks ----
    let drift = down * wind_ms * 0.9;
    for (src, strength) in &sources {
        if view.puffs.len() < MAX_SMOKE && view.rand() < 0.035 * strength {
            let jitter = Vec3::new(view.rand() - 0.5, 0.0, view.rand() - 0.5) * 12.0;
            let life = 22.0 + 20.0 * view.rand();
            let size = 14.0 + 10.0 * strength + 8.0 * view.rand();
            let shade = (0.42 - 0.12 * strength.min(1.5)).max(0.18);
            view.puffs.push(Puff { pos: *src + jitter + Vec3::Y * 10.0, vel: Vec3::Y * (5.0 + 3.0 * strength) + drift * 0.3, age: 0.0, life, size, shade });
        }
        if view.embers.len() < MAX_SPARKS && view.rand() < 0.03 * strength {
            let v = Vec3::new(view.rand() - 0.5, 0.0, view.rand() - 0.5) * 4.0 + Vec3::Y * (6.0 + 6.0 * view.rand());
            let life = 1.5 + 2.0 * view.rand();
            view.embers.push(Spark { pos: *src, vel: v + down * wind_ms * 0.5, age: 0.0, life });
        }
    }
    for p in &mut view.puffs {
        p.age += dt;
        let k = p.age / p.life;
        // Buoyant rise slows as the plume cools; the wind takes over.
        p.vel = p.vel.lerp(drift + Vec3::Y * 1.2, (dt * 0.25).min(1.0));
        p.pos += p.vel * dt * (1.0 + 0.3 * k);
    }
    view.puffs.retain(|p| p.age < p.life);
    for e in &mut view.embers {
        e.age += dt;
        e.vel += (drift - e.vel) * (dt * 0.8).min(1.0) + Vec3::Y * (-2.5 * dt);
        e.pos += e.vel * dt;
    }
    view.embers.retain(|e| e.age < e.life);

    // Smoke and sparks are true camera-facing billboards.
    let (cam_right, cam_up) = (cam.rotation * Vec3::X, cam.rotation * Vec3::Y);
    let mut smoke = QuadBuilder::default();
    for p in &view.puffs {
        let k = (p.age / p.life).clamp(0.0, 1.0);
        let size = p.size * (0.6 + 2.2 * k);
        let alpha = (k * 5.0).min(1.0) * (1.0 - k).powf(1.3) * 0.22;
        let sh = p.shade + 0.35 * k;
        let r = cam_right * size;
        let u = cam_up * size;
        let c = [sh, sh * 0.96, sh * 0.92, alpha];
        smoke.quad([p.pos - r - u, p.pos + r - u, p.pos + r + u, p.pos - r + u], c, c);
    }
    meshes.insert(&view.smoke, smoke.finish());
    let mut sparks = QuadBuilder::default();
    for e in &view.embers {
        let fade = 1.0 - e.age / e.life;
        let size = 0.9 + 0.8 * fade;
        let c = [3.0 * fade, 1.1 * fade, 0.2 * fade, 1.0];
        let (r, u) = (cam_right * size, cam_up * size);
        sparks.quad([e.pos - r - u, e.pos + r - u, e.pos + r + u, e.pos - r + u], c, c);
    }
    meshes.insert(&view.sparks, sparks.finish());
}

const TEX: u32 = 64;

fn image(pixels: Vec<u8>) -> Image {
    Image::new(Extent3d { width: TEX, height: TEX, depth_or_array_layers: 1 }, TextureDimension::D2, pixels, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default())
}

fn tex_noise(x: f32, y: f32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let h = |a: f32, b: f32| hash01(((a as i64 * 73_856_093) ^ (b as i64 * 19_349_663)) as u64);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let top = h(xi, yi) + (h(xi + 1.0, yi) - h(xi, yi)) * sx;
    let bot = h(xi, yi + 1.0) + (h(xi + 1.0, yi + 1.0) - h(xi, yi + 1.0)) * sx;
    top + (bot - top) * sy
}

/// A flame tongue: red channel = hot core (used by the shader's colour ramp),
/// alpha = shape. v = 0 at the tip, 1 at the base.
fn tongue_texture() -> Image {
    let mut px = Vec::with_capacity((TEX * TEX * 4) as usize);
    for y in 0..TEX {
        let v = y as f32 / (TEX - 1) as f32;
        for x in 0..TEX {
            let u = x as f32 / (TEX - 1) as f32 * 2.0 - 1.0;
            let width = (0.18 + 0.82 * v.powf(0.8)).max(0.06);
            let edge = (1.0 - (u.abs() / width).powf(1.8)).clamp(0.0, 1.0);
            let turb = 0.6 + 0.8 * tex_noise(u * 3.0 + 11.0, v * 6.0);
            // Fade the very base so a tongue never shows a cut line at the ground.
            let foot = (1.0 - (v - 0.92).max(0.0) * 12.0).clamp(0.0, 1.0);
            let a = (edge * turb * foot * (0.3 + 0.8 * v)).clamp(0.0, 1.0);
            let core = (edge.powf(2.0) * (0.3 + 0.7 * v)).clamp(0.0, 1.0);
            px.extend_from_slice(&[(core * 255.0) as u8, 0, 0, (a * 255.0) as u8]);
        }
    }
    image(px)
}

/// A soft, lumpy puff for smoke and sparks.
fn puff_texture() -> Image {
    let mut px = Vec::with_capacity((TEX * TEX * 4) as usize);
    for y in 0..TEX {
        let v = y as f32 / (TEX - 1) as f32 * 2.0 - 1.0;
        for x in 0..TEX {
            let u = x as f32 / (TEX - 1) as f32 * 2.0 - 1.0;
            let r = (u * u + v * v).sqrt() * (0.8 + 0.4 * tex_noise(u * 2.5 + 3.0, v * 2.5 + 7.0));
            let a = (1.0 - r).clamp(0.0, 1.0).powf(1.5);
            px.extend_from_slice(&[255, 255, 255, (a * 255.0) as u8]);
        }
    }
    image(px)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flame_height_grows_with_intensity_and_probability() {
        let lo = tongue_for(500.0, 1.0, 1.0, false, 0.5).height;
        let hi = tongue_for(8000.0, 1.0, 1.0, false, 0.5).height;
        assert!(hi > lo * 1.5, "{lo} vs {hi}");
        let unsure = tongue_for(8000.0, 0.55, 1.0, false, 0.5).height;
        assert!(unsure < hi);
        // Halo tongues are short and translucent, brighter with probability.
        let h1 = tongue_for(8000.0, 0.1, 1.0, true, 0.5);
        let h2 = tongue_for(8000.0, 0.45, 1.0, true, 0.5);
        assert!(h2.height > h1.height && h2.alpha > h1.alpha && h2.height < unsure);
        // Flames shrink as the cell moves out of the flaming band.
        assert!(tongue_for(8000.0, 1.0, 0.1, false, 0.5).height < hi);
    }

    #[test]
    fn wind_tilts_flames() {
        assert_eq!(tilt_angle(0.0, 4.0), 0.0);
        let calm = tilt_angle(2.0, 4.0);
        let strong = tilt_angle(12.5, 4.0);
        assert!(strong > calm);
        assert!(strong <= 65f32.to_radians() + 1e-6);
        assert!(drawn_tilt(12.5, 4.0) < strong && drawn_tilt(40.0, 1.0) <= 50f32.to_radians() + 1e-6);
        // Taller flames tilt less at the same wind.
        assert!(tilt_angle(12.5, 10.0) < strong);
    }
}

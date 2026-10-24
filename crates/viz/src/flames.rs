//! Flames, smoke and sparks, driven by the Monte Carlo fire.
//!
//! Uses propagator_abm's seven-segment ribbons, flame/ember/smoke/ground
//! shaders, procedural textures and CPU puff fog, adapted to TTX snapshots:
//!
//! - **Intensity → flame length.** Each tongue's height is Byram's flame length
//!   `L = 0.0775·I^0.46` of the cell's *ensemble-mean* fireline intensity (mean
//!   over the runs that reached it), times one constant visual exaggeration
//!   ([`FLAME_EXAGGERATION`]) so a 4 m flame reads from 2 km. Relative
//!   heights stay physical.
//! - **Probability → confidence.** In the actual fire (cells reached by ≥ cutoff
//!   of the runs) the height and the number of tongues scale with the fraction
//!   of runs that burned the cell. In the uncertainty halo (reached by only
//!   some runs) short, dim tongues show where the fire *might* be,
//!   with brightness proportional to probability.
//! - **Wind tilt.** Tongues lean downwind by `θ = atan(U / √(g·L))` (a Froude-number
//!   tilt, capped at 65°), so a wind-driven head fire lies over while a flank
//!   stands up.
//! - **Life cycle.** A cell flames for [`FLAMING_S`] after its (quantile)
//!   arrival time, strongest in the first minutes; the ground then glows and
//!   cools to a scar (see `terrain.rs`) and smokes.
//!
//! Ribbons and particle geometry are rebuilt every frame, so the fire
//! stays alive between simulation updates and at any speed.

use crate::fire_shader::{FireGroundMaterial, FireMaterial, FireShaderPlugin, SmokeMaterial};
use crate::fire_textures as textures;
use crate::frame::{height_at, to_bevy, EXAGGERATION};
use crate::{Scn, Snapshot};
use bevy::pbr::{FogFalloff, FogSettings};
use bevy::prelude::*;
use bevy::render::camera::Exposure;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use scenario::{Cell, Pos};

/// Visual exaggeration of flame height (real flames here are 1–6 m).
pub const FLAME_EXAGGERATION: f32 = 9.0;
/// Seconds a cell flames after the front arrives (the rest of its 20-minute
/// residence time is glowing and smouldering).
pub const FLAMING_S: f32 = 420.0;
/// Seconds a burnt-out cell keeps smoking.
const SMOKING_S: f32 = 1800.0;
const MAX_TONGUES: usize = 4000;
const MAX_SMOKE: usize = 1600;
const MAX_SPARKS: usize = 500;
const G: f32 = 9.81;

pub struct FlamesPlugin;

impl Plugin for FlamesPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FireShaderPlugin)
            .add_systems(Startup, setup.after(crate::terrain::setup))
            .add_systems(
                Update,
                update
                    .after(crate::frame::update)
                    .after(crate::fire_field::toggle),
            );
    }
}

struct Puff {
    pos: Vec3,
    vel: Vec3,
    age: f32,
    life: f32,
    size: f32,
    /// Height of the ground where it was emitted (Bevy units), for wind shear.
    base_y: f32,
    phase: f32,
}

struct Spark {
    pos: Vec3,
    vel: Vec3,
    age: f32,
    life: f32,
    size: f32,
}

#[derive(Resource)]
pub struct FlameView {
    ground: Handle<Mesh>,
    ground_key: Option<(u64, i64, bool)>,
    last_clock: Option<(i64, u32)>,
    flames: Handle<Mesh>,
    smoke: Handle<Mesh>,
    sparks: Handle<Mesh>,
    puffs: Vec<Puff>,
    embers: Vec<Spark>,
    rng: u64,
    /// Fractional puffs owed by the emission-rate accumulator.
    emit_acc: f32,
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

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut fire_mats: ResMut<Assets<FireMaterial>>,
    mut smoke_mats: ResMut<Assets<SmokeMaterial>>,
    mut ground_mats: ResMut<Assets<FireGroundMaterial>>,
    terrain: Res<crate::terrain::TerrainHandles>,
    scn: Res<Scn>,
) {
    let ground = meshes.add(empty_mesh());
    let flames = meshes.add(empty_mesh());
    let smoke = meshes.add(empty_mesh());
    let sparks = meshes.add(empty_mesh());
    let flame_tex = images.add(textures::flame_tongue());
    let puff_tex = images.add(textures::puff());
    let spark_tex = images.add(textures::spark());
    use bevy::pbr::NotShadowCaster;
    use bevy::render::view::NoFrustumCulling;
    commands.spawn((
        MaterialMeshBundle {
            mesh: ground.clone(),
            material: ground_mats.add(FireGroundMaterial {
                road_mask: Some(terrain.road_mask.clone()),
                fire_state: terrain.state.clone(),
                mask_rect: Vec4::new(
                    1. / scn.0.grid.width_m(),
                    1. / scn.0.grid.height_m(),
                    0.,
                    0.,
                ),
            }),
            ..default()
        },
        NotShadowCaster,
        NoFrustumCulling,
    ));
    commands.spawn((
        MaterialMeshBundle {
            mesh: flames.clone(),
            material: fire_mats.add(FireMaterial {
                texture: flame_tex,
                mode: Vec4::X,
            }),
            ..default()
        },
        NotShadowCaster,
        NoFrustumCulling,
    ));
    commands.spawn((
        MaterialMeshBundle {
            mesh: smoke.clone(),
            material: smoke_mats.add(SmokeMaterial { texture: puff_tex }),
            ..default()
        },
        NotShadowCaster,
        NoFrustumCulling,
    ));
    commands.spawn((
        MaterialMeshBundle {
            mesh: sparks.clone(),
            material: fire_mats.add(FireMaterial {
                texture: spark_tex,
                mode: Vec4::new(2., 0., 0., 0.),
            }),
            ..default()
        },
        NotShadowCaster,
        NoFrustumCulling,
    ));
    commands.insert_resource(FlameView {
        ground,
        ground_key: None,
        last_clock: None,
        flames,
        smoke,
        sparks,
        puffs: Vec::new(),
        embers: Vec::new(),
        rng: 0x9E37_79B9_7F4A_7C15,
        emit_acc: 0.,
    });
}

fn empty_mesh() -> Mesh {
    QuadBuilder::default().finish()
}

/// Rows per flame tongue ribbon.
const TONGUE_SEGMENTS: usize = 7;

#[derive(Default)]
struct QuadBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
    /// Flames and embers read no normal (their shader never lights); only the
    /// smoke uses the slot, for its fog.
    no_normals: bool,
}

impl QuadBuilder {
    fn with_capacity(verts: usize) -> Self {
        QuadBuilder {
            positions: Vec::with_capacity(verts),
            normals: Vec::with_capacity(verts),
            colors: Vec::with_capacity(verts),
            uvs: Vec::with_capacity(verts),
            indices: Vec::with_capacity(verts / 16 * 42),
            no_normals: true,
        }
    }

    /// A textured billboard. `right`/`up` are world-space half-extents;
    /// `taper` narrows the top edge, which is what gives a flame tongue its
    /// shape independently of the texture.
    fn billboard(
        &mut self,
        centre: Vec3,
        right: Vec3,
        up: Vec3,
        taper: f32,
        bottom: [f32; 4],
        top: [f32; 4],
    ) {
        let base = self.positions.len() as u32;
        for (offset, color, uv) in [
            (centre - right - up, bottom, [0.0, 1.0]),
            (centre + right - up, bottom, [1.0, 1.0]),
            (centre + right * taper + up, top, [1.0, 0.0]),
            (centre - right * taper + up, top, [0.0, 0.0]),
        ] {
            self.positions.push(offset.into());
            if !self.no_normals {
                self.normals.push([0.0, 1.0, 0.0]);
            }
            self.colors.push(color);
            self.uvs.push(uv);
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// A flame tongue: a ribbon of `TONGUE_SEGMENTS` rows from foot to tip.
    /// It tapers to a point and its spine travels on two travelling waves
    /// whose amplitude grows toward the tip, so the tongue snakes and licks
    /// instead of bobbing. `foot` is the centre of the base; `h` the height.
    #[allow(clippy::too_many_arguments)]
    fn tongue(
        &mut self,
        foot: Vec3,
        right: Vec3,
        h: f32,
        half_w: f32,
        t: f32,
        phase: f32,
        lean: Vec3,
        bottom: [f32; 4],
        top: [f32; 4],
    ) {
        let base = self.positions.len() as u32;
        for j in 0..=TONGUE_SEGMENTS {
            let s = j as f32 / TONGUE_SEGMENTS as f32; // 0 foot -> 1 tip
            let s2 = s * s;
            // Two waves running upward at different speeds: the second makes
            // the motion non-periodic to the eye.
            let w1 = (t * 7.0 - s * 4.5 + phase).sin();
            let w2 = (t * 11.3 - s * 7.0 + phase * 1.7).sin();
            let lateral = h * (0.30 * s2 * w1 + 0.12 * s2 * s * w2);
            // The tip droops a little as it whips.
            let drop = h * 0.06 * s2 * s * (w1 * w2).abs();
            let width =
                half_w * (1.0 - s).powf(0.85) * (1.0 + 0.18 * (t * 9.0 + phase + s * 3.0).sin());
            let centre = foot + Vec3::Y * (h * s - drop) + right * lateral + lean * s2;
            let color = [
                bottom[0] + (top[0] - bottom[0]) * s,
                bottom[1] + (top[1] - bottom[1]) * s,
                bottom[2] + (top[2] - bottom[2]) * s,
                bottom[3] + (top[3] - bottom[3]) * s,
            ];
            // uv.y: 1 at the foot, 0 at the tip (what the shader expects).
            for (side, u) in [(-1.0f32, 0.0f32), (1.0, 1.0)] {
                self.positions.push((centre + right * width * side).into());
                if !self.no_normals {
                    self.normals.push([0.0, 1.0, 0.0]);
                }
                self.colors.push(color);
                self.uvs.push([u, 1.0 - s]);
            }
        }
        for j in 0..TONGUE_SEGMENTS as u32 {
            let a = base + j * 2;
            self.indices
                .extend_from_slice(&[a, a + 1, a + 3, a, a + 3, a + 2]);
        }
    }

    fn finish(self) -> Mesh {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        if !self.no_normals {
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        }
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        mesh.insert_indices(Indices::U32(self.indices));
        mesh
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
    (wind_ms / (G * flame_m).sqrt())
        .atan()
        .min(65f32.to_radians())
}

/// The tilt as drawn: 50 % of the physical angle, at most 35°. A 60°-tilted
/// tongue seen from above is a streak on the ground; this keeps the lean
/// readable while strong winds still visibly lay the head fire over.
pub fn drawn_tilt(wind_ms: f32, flame_m: f32) -> f32 {
    (tilt_angle(wind_ms, flame_m) * 0.5).min(35f32.to_radians())
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
    let l = view::flame_length_m(fli).max(1.2);
    if halo {
        let conf = (p / cutoff.max(1e-3)).clamp(0.0, 1.0);
        let height = l * FLAME_EXAGGERATION * 0.35 * (0.4 + 0.6 * conf);
        return Tongue {
            height,
            half_width: (height * 0.34).min(12.0),
            heat: 0.55,
            alpha: 0.25 + 0.45 * conf,
        };
    }
    let height = l * FLAME_EXAGGERATION * (0.45 + 0.55 * flaming) * (0.55 + 0.45 * p);
    let heat = (fli / 3000.0).clamp(0.6, 2.2) * (0.55 + 0.45 * flaming);
    Tongue {
        height,
        half_width: (height * 0.34).min(14.0),
        heat,
        alpha: 1.0,
    }
}

#[allow(clippy::too_many_arguments)]
fn update(
    snap: Res<Snapshot>,
    quality: Res<crate::quality::Quality>,
    style: Res<crate::fire_field::FirePresentation>,
    scn: Res<Scn>,
    view: Option<ResMut<FlameView>>,
    mut meshes: ResMut<Assets<Mesh>>,
    cam: Query<&Transform, With<Camera3d>>,
    time: Res<Time>,
    fog: Query<(&FogSettings, Option<&Exposure>), With<Camera3d>>,
    suns: Query<(&GlobalTransform, &DirectionalLight)>,
) {
    let (Some(mut view), Ok(cam)) = (view, cam.get_single()) else {
        return;
    };
    let s = &scn.0;
    let n = s.grid.len();
    if snap.cells.len() != n || snap.intensity.len() != n {
        return;
    }
    if view
        .last_clock
        .is_some_and(|(last, turn)| snap.time_s < last || snap.turn < turn)
    {
        view.puffs.clear();
        view.embers.clear();
        view.emit_acc = 0.;
        view.rng = 0x9E37_79B9_7F4A_7C15;
        view.ground_key = None;
    }
    view.last_clock = Some((snap.time_s, snap.turn));
    if view.ground_key != Some((snap.generation, snap.time_s, style.continuous)) {
        meshes.insert(
            &view.ground,
            if style.continuous {
                ground_overlay_continuous(s, &snap, &quality)
            } else {
                ground_overlay(s, &snap)
            },
        );
        view.ground_key = Some((snap.generation, snap.time_s, style.continuous));
    }
    let t = time.elapsed_seconds();
    let dt = time.delta_seconds().min(0.1);
    let now = snap.time_s as f32;
    let field = crate::fire_field::FireField {
        snap: &snap,
        grid: s.grid,
    };
    let cutoff = snap.ensemble.map(|e| e.cutoff).unwrap_or(0.5);
    let w = snap.weather.unwrap_or(s.weather);
    let wind_ms = w.wind_speed_kmh as f32 / 3.6;
    // Wind blows *from* wind_dir; downwind unit vector in the world frame (x east, y north).
    let to = (w.wind_dir_deg as f32 + 180.0).to_radians();
    let down_world = Vec2::new(to.sin(), to.cos());
    let down = Vec3::new(down_world.x, 0.0, -down_world.y);
    // Upright billboards facing the camera horizontally.
    let mut right = cam.rotation * Vec3::X;
    right.y = 0.0;
    let right = right.normalize_or_zero();

    let mut flames = QuadBuilder::with_capacity(MAX_TONGUES * 16);
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
            if code == 2
                && snap.arrival[i] != i32::MIN
                && now - (snap.arrival[i] as f32) < SMOKING_S
            {
                // Recently burnt: smoulder, feeding the plume a little.
                if hash01(i as u64 * 31 + (t * 2.0) as u64) < 0.004 {
                    let c = s.cell_centre(Cell {
                        row: i / cols,
                        col: i % cols,
                    });
                    sources.push((to_bevy(s, c, 2.0), 0.25));
                }
            }
            continue;
        }
        let fli = snap.intensity[i];
        let flaming = if burning && snap.arrival[i] != i32::MIN {
            (1.0 - (now - snap.arrival[i] as f32) / FLAMING_S).clamp(0.0, 1.0)
        } else {
            0.6
        };
        if burning && flaming < 0.03 {
            // Past the flaming band: glowing ground (terrain.rs) and smoke.
            if hash01(i as u64 * 17 + (t * 3.0) as u64) < 0.01 {
                let c = s.cell_centre(Cell {
                    row: i / cols,
                    col: i % cols,
                });
                sources.push((to_bevy(s, c, 2.0), 0.5));
            }
            continue;
        }
        let count = if halo {
            1
        } else {
            (1.0 + 3.0 * p * (fli / 3000.0).min(1.5) * flaming).round() as usize
        };
        let centre = s.cell_centre(Cell {
            row: i / cols,
            col: i % cols,
        });
        for k in 0..count {
            if tongues >= MAX_TONGUES {
                break;
            }
            tongues += 1;
            let h1 = hash01(i as u64 * 7919 + k as u64 * 31);
            let h2 = hash01(i as u64 * 104_729 + k as u64 * 97);
            let pos: Pos = [
                centre[0] + (h1 - 0.5) * 2.0 * half_cell,
                centre[1] + (h2 - 0.5) * 2.0 * half_cell,
            ];
            let base = if style.continuous && !halo {
                let sample = field.sample(pos);
                let Some(arrival) = sample.arrival else {
                    continue;
                };
                let local_flaming = (1. - (now - arrival) / FLAMING_S).clamp(0., 1.);
                if sample.coverage < 0.5 || arrival > now || local_flaming < 0.03 {
                    continue;
                }
                tongue_for(sample.intensity, p, local_flaming, false, cutoff)
            } else {
                tongue_for(fli, p, flaming, halo, cutoff)
            };
            let ground = Vec3::new(
                pos[0] - s.grid.width_m() * 0.5,
                height_at(s, pos) * EXAGGERATION - 0.5,
                -(pos[1] - s.grid.height_m() * 0.5),
            );
            let phase = h1 * 40.0 + k as f32 * 2.1;
            let flicker = 0.7 + 0.3 * (t * 5.3 + phase).sin() * (t * 2.1 + phase * 0.7).cos();
            let h = base.height * flicker * (0.75 + 0.5 * h2);
            let hw = base.half_width * (0.8 + 0.4 * h1);
            // Froude tilt on the real flame length, applied to the drawn height.
            let theta = drawn_tilt(wind_ms, h / FLAME_EXAGGERATION);
            let lean = down * (theta.tan() * h) + right * ((t * 2.7 + phase).sin() * h * 0.06);
            let heat = base.heat;
            let a = base.alpha;
            let (bottom, top) = if halo {
                (
                    [2.2 * heat, 0.55 * heat, 0.12 * heat, a],
                    [1.3 * heat, 0.22 * heat, 0.05 * heat, a],
                )
            } else {
                (
                    [3.4 * heat, 1.5 * heat, 0.3 * heat, a],
                    [2.1 * heat, 0.55 * heat, 0.08 * heat, a],
                )
            };
            // Source game's travelling-wave ribbon, rather than a rigid quad.
            // Keep ensemble confidence and TTX wind tilt in its vertex data.
            flames.tongue(ground, right, h, hw, t, phase, lean, bottom, top);
            if !halo && k == 0 {
                sources.push((
                    ground + Vec3::Y * h * 0.8 + lean * 0.8,
                    (fli / 3000.0).clamp(0.3, 2.0) * flaming.max(0.3),
                ));
            }
        }
    }
    meshes.insert(&view.flames, flames.finish());

    // ---- smoke and sparks ----
    // The plume: puffs rise buoyantly off the fire, cool and slow, and are
    // carried downwind. The wind speeds up with height (a log-like profile),
    // so the column bends over and trails away from the fire; gusts and a
    // little turbulence keep it from looking like a conveyor belt.
    let gust = 1.0 + 0.25 * (t * 0.37).sin() + 0.12 * (t * 1.13).sin();
    let wind_at =
        |height: f32| down * wind_ms * gust * (0.55 + 0.45 * (height / 200.0).clamp(0.0, 2.0));
    // Emit at a steady rate matched to the particle budget (so the plume stays
    // attached to the fire), choosing sources in proportion to their strength.
    const PUFF_LIFE: f32 = 38.0;
    let max_strength = sources.iter().map(|(_, k)| *k).fold(0.0f32, f32::max);
    if max_strength > 0.0 {
        view.emit_acc +=
            (sources.len() as f32 * 1.2).min(MAX_SMOKE as f32 * quality.vegetation) * dt;
        let mut tries = 0;
        while view.emit_acc >= 1.0 && tries < 200 {
            tries += 1;
            let (src, strength) =
                sources[((view.rand() * sources.len() as f32) as usize).min(sources.len() - 1)];
            if view.rand() * max_strength > strength {
                continue;
            }
            view.emit_acc -= 1.0;
            if view.puffs.len() >= (MAX_SMOKE as f32 * quality.vegetation) as usize {
                continue;
            }
            let jitter = Vec3::new(view.rand() - 0.5, 0.0, view.rand() - 0.5) * 14.0;
            let life = PUFF_LIFE * (0.8 + 0.4 * view.rand());
            let size = 10.0 + 8.0 * strength.min(2.0) + 6.0 * view.rand();
            let phase = view.rand() * 100.0;
            let rise = 3.5 + 2.5 * strength.min(2.0) + 1.5 * view.rand();
            view.puffs.push(Puff {
                pos: src + jitter + Vec3::Y * 6.0,
                vel: Vec3::Y * rise + wind_at(0.0) * 0.5,
                age: 0.0,
                life,
                size,
                base_y: src.y,
                phase,
            });
        }
        view.emit_acc = view.emit_acc.min(5.0);
    }
    for (src, strength) in &sources {
        if view.embers.len() < (MAX_SPARKS as f32 * quality.vegetation) as usize
            && view.rand() < dt * 0.375 * strength
        {
            let v = Vec3::new(view.rand() - 0.5, 0.0, view.rand() - 0.5) * 4.0
                + Vec3::Y * (6.0 + 6.0 * view.rand());
            let life = 1.5 + 2.0 * view.rand();
            view.embers.push(Spark {
                pos: *src,
                vel: v + down * wind_ms * 0.5,
                age: 0.0,
                life,
                size: 0.9,
            });
        }
    }
    let drift = wind_at(120.0);
    for p in &mut view.puffs {
        p.age += dt;
        let h = p.pos.y - p.base_y;
        // Buoyancy decays as the smoke cools; horizontal speed relaxes to the wind at this height.
        let buoy = (p.vel.y * (1.0 - dt * 0.08)).max(0.8);
        let horiz = Vec3::new(p.vel.x, 0.0, p.vel.z).lerp(wind_at(h), (dt * 0.6).min(1.0));
        let across = Vec3::new(-down.z, 0.0, down.x);
        let turb = across * ((t * 0.5 + p.phase).sin() * 1.5)
            + Vec3::Y * ((t * 0.7 + p.phase * 1.3).cos() * 0.6);
        p.vel = Vec3::new(horiz.x, buoy, horiz.z);
        p.pos += (p.vel + turb) * dt;
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
    let fogger = PuffFog::new(&fog, &suns, cam.translation);
    let mut smoke = QuadBuilder {
        no_normals: false,
        ..default()
    };
    for puff in &view.puffs {
        let k = (puff.age / puff.life).clamp(0.0, 1.0);
        // Grows as it disperses, darkest and densest near the fire.
        let size = puff.size * (0.6 + 1.9 * k);
        // Pale, toy smoke: grey near the fire, near-white as it disperses, so
        // the plume shows the wind without burying the flames under it.
        let shade = 0.58 + 0.34 * k;
        // Fade in fast, out slowly: a puff should never pop into existence.
        // Kept thin — hundreds of puffs overlap, and at 0.55 each the plume
        // turned into a white wall as soon as the camera dropped into it.
        let alpha = (k * 4.0).min(1.0) * (1.0 - k).powf(1.4) * 0.22;
        let spin = puff.phase * std::f32::consts::TAU + k * 0.6;
        let (s, c) = spin.sin_cos();
        let r = (cam_right * c + cam_up * s) * size;
        let u = (cam_up * c - cam_right * s) * size;
        let (gain, offset) = fogger.at(puff.pos);
        let c = [
            shade * 1.05 * gain.x,
            shade * 0.92 * gain.y,
            shade * 0.84 * gain.z,
            alpha,
        ];
        smoke.billboard(puff.pos, r, u, 1.0, c, c);
        // The normal slot carries the fog's additive part (`smoke.wgsl`).
        for n in smoke.normals.iter_mut().rev().take(4) {
            *n = offset.into();
        }
    }

    meshes.insert(&view.smoke, smoke.finish());
    let eye = cam.translation;
    let mut sparks = QuadBuilder {
        no_normals: true,
        ..default()
    };
    for ember in &view.embers {
        let phase = hash01(ember.life.to_bits() as u64);
        let k = (ember.age / ember.life).clamp(0.0, 1.0);
        // never smaller than ~2 px, or a far ember shimmers in and out
        let min_size = eye.distance(ember.pos) * 0.0017;
        let size = (ember.size * (1.0 - 0.35 * k)).max(min_size);
        let gain = 0.9 + 0.5 * phase;
        let c = [k, phase, gain, 1.0];
        // An oval: a little longer along the direction of travel, seen from
        // the camera. The shader draws the round-edged shape inside it.
        let to_eye = (eye - ember.pos).normalize_or_zero();
        let speed = ember.vel.length();
        let dir = if speed > 0.5 {
            ember.vel / speed
        } else {
            Vec3::Y
        };
        let mut across = dir.cross(to_eye).normalize_or_zero();
        let mut along = across.cross(to_eye).normalize_or_zero();
        if across == Vec3::ZERO || along == Vec3::ZERO {
            across = cam_right;
            along = cam_up;
        }
        let stretch = 1.0 + (0.04 * speed).min(0.7);
        sparks.billboard(ember.pos, across * size, along * size * stretch, 1.0, c, c);
    }

    meshes.insert(&view.sparks, sparks.finish());
}

/// The scene's distance fog as `colour * gain + offset`, evaluated on the CPU
/// for one puff. `bevy_pbr::fog::atmospheric_fog` is affine in the colour it
/// fogs, and across a puff the distance and view direction barely change, so
/// one evaluation per puff replaces a per-pixel one (a light loop and a pow
/// per fragment, about a fifth of the smoke's GPU time). Only the atmospheric
/// falloff is mirrored, the one `sky.rs` and `tactical.rs` use; with any other
/// the puffs go unfogged.
struct PuffFog {
    eye: Vec3,
    /// (extinction, inscattering, base colour, light colour x alpha) when fog is on.
    atmo: Option<(Vec3, Vec3, Vec4, Vec4, f32)>,
    /// Direction to each directional light, and its colour x exposure.
    lights: Vec<(Vec3, Vec3)>,
}

impl PuffFog {
    fn new(
        fog: &Query<(&FogSettings, Option<&Exposure>), With<Camera3d>>,
        suns: &Query<(&GlobalTransform, &DirectionalLight)>,
        eye: Vec3,
    ) -> Self {
        let mut out = PuffFog {
            eye,
            atmo: None,
            lights: Vec::new(),
        };
        let Ok((f, exposure)) = fog.get_single() else {
            return out;
        };
        let FogFalloff::Atmospheric {
            extinction,
            inscattering,
        } = f.falloff
        else {
            return out;
        };
        let exposure = exposure.copied().unwrap_or_default().exposure();
        let base = LinearRgba::from(f.color);
        let dl = LinearRgba::from(f.directional_light_color);
        out.atmo = Some((
            extinction,
            inscattering,
            Vec4::new(base.red, base.green, base.blue, base.alpha),
            Vec4::new(dl.red, dl.green, dl.blue, dl.alpha),
            f.directional_light_exponent,
        ));
        for (t, l) in suns.iter() {
            let c = LinearRgba::from(l.color) * l.illuminance * exposure;
            out.lights
                .push((t.back().into(), Vec3::new(c.red, c.green, c.blue)));
        }
        out
    }

    /// (gain, offset) for a puff at `p`.
    fn at(&self, p: Vec3) -> (Vec3, Vec3) {
        let Some((be, bi, base, dl, exponent)) = self.atmo else {
            return (Vec3::ONE, Vec3::ZERO);
        };
        let v = p - self.eye;
        let d = v.length();
        let dir = v / d.max(1e-3);
        let mut fog = Vec3::new(base.x, base.y, base.z);
        if dl.w > 0.0 {
            let mut scatter = Vec3::ZERO;
            for (to_light, colour) in &self.lights {
                scatter += dir.dot(*to_light).max(0.0).powf(exponent) * *colour;
            }
            fog += scatter * Vec3::new(dl.x, dl.y, dl.z) * dl.w;
        }
        let ext = Vec3::ONE - Vec3::new((-d * be.x).exp(), (-d * be.y).exp(), (-d * be.z).exp());
        let ins = Vec3::ONE - Vec3::new((-d * bi.x).exp(), (-d * bi.y).exp(), (-d * bi.z).exp());
        (Vec3::ONE - ext * base.w, fog * ins * base.w)
    }
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
        assert!(
            drawn_tilt(12.5, 4.0) < strong && drawn_tilt(40.0, 1.0) <= 35f32.to_radians() + 1e-6
        );
        // Taller flames tilt less at the same wind.
        assert!(tilt_angle(12.5, 10.0) < strong);
    }
}

/// Source game's glow/ash colour convention, sampled only inside confirmed
/// TTX fire cells. Independent quads keep the ensemble cutoff exact; shader
/// grain cannot create extra burning cells or an unapproved forecast perimeter.
fn ground_overlay(s: &scenario::Scenario, snap: &Snapshot) -> Mesh {
    let mut mesh = QuadBuilder::default();
    for (i, &code) in snap.cells.iter().enumerate() {
        if code != 1 && code != 2 {
            continue;
        }
        let centre = s.cell_centre(Cell {
            row: i / s.grid.cols,
            col: i % s.grid.cols,
        });
        let half = s.grid.cellsize * 0.5;
        // Drape on the same continuous visual heightfield as the terrain.
        for row in 0..2 {
            for col in 0..2 {
                let x = centre[0] - half + col as f32 * half;
                let y = centre[1] + half - row as f32 * half;
                let corners = [[x, y - half], [x + half, y - half], [x + half, y], [x, y]];
                let off = mesh.positions.len() as u32;
                for p in corners {
                    mesh.positions.push(to_bevy(s, p, 0.35).to_array());
                    mesh.normals.push([0., 1., 0.]);
                    mesh.colors.push(ground_colour(s, snap, p));
                    mesh.uvs.push([0., 0.]);
                }
                mesh.indices
                    .extend([off, off + 1, off + 2, off, off + 2, off + 3]);
            }
        }
    }
    mesh.finish()
}

#[cfg(test)]
mod transfer_tests {
    use super::*;
    #[test]
    fn source_ribbon_has_seven_finite_sections_and_preserves_confidence() {
        let mut b = QuadBuilder::with_capacity(16);
        b.tongue(
            Vec3::ZERO,
            Vec3::X,
            40.,
            10.,
            1.2,
            0.7,
            Vec3::Z * 6.,
            [3., 1., 0.2, 0.25],
            [2., 0.5, 0.1, 0.25],
        );
        assert_eq!(b.positions.len(), 16);
        assert_eq!(b.indices.len(), 42);
        assert!(b.positions.iter().all(|p| Vec3::from_array(*p).is_finite()));
        assert!(b.colors.iter().all(|c| c[3] == 0.25));
        assert!(b.positions.last().unwrap()[2] > 5.9);
        assert_eq!(b.positions[14], b.positions[15]); // tapered tip
    }
    #[test]
    fn ground_glow_never_draws_uncertainty_or_cleared_fuel() {
        let s = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
        let mut snap = Snapshot {
            cells: vec![0; s.grid.len()],
            arrival: vec![0; s.grid.len()],
            ..default()
        };
        snap.cells[10] = 5;
        snap.cells[11] = 3;
        let mesh = ground_overlay(&s, &snap);
        assert_eq!(mesh.count_vertices(), 0);
        snap.cells[12] = 1;
        assert_eq!(ground_overlay(&s, &snap).count_vertices(), 16);
        snap.cells[12] = 0; // restored snapshot removes its glow as well
        assert_eq!(ground_overlay(&s, &snap).count_vertices(), 0);
    }
}

/// Bilinear source-style heat/coverage, clipped by the confirmed parent cells
/// in ground_overlay. This is shading, not a finer-resolution fire forecast.
fn ground_colour(s: &scenario::Scenario, snap: &Snapshot, p: Pos) -> [f32; 4] {
    let g = s.grid;
    let x = (p[0] / g.cellsize - 0.5).clamp(0., (g.cols - 1) as f32);
    let y = ((g.height_m() - p[1]) / g.cellsize - 0.5).clamp(0., (g.rows - 1) as f32);
    let (col, row) = (x.floor() as usize, y.floor() as usize);
    let (u, v) = (x.fract(), y.fract());
    let mut heat = 0.;
    let mut ember = 0.;
    let mut cover = 0.;
    for (r, c, w) in [
        (row, col, (1. - u) * (1. - v)),
        (row, (col + 1).min(g.cols - 1), u * (1. - v)),
        ((row + 1).min(g.rows - 1), col, (1. - u) * v),
        ((row + 1).min(g.rows - 1), (col + 1).min(g.cols - 1), u * v),
    ] {
        let i = r * g.cols + c;
        let code = snap.cells[i];
        if code != 1 && code != 2 {
            continue;
        }
        let arrival = snap
            .arrival
            .get(i)
            .copied()
            .filter(|a| *a != i32::MIN)
            .unwrap_or(snap.time_s as i32);
        let age = (snap.time_s as f32 - arrival as f32).max(0.);
        if code == 1 {
            heat += w * (1. - age / 1100.).clamp(0., 1.).powi(2);
        }
        ember += w * (1. - age / 4400.).clamp(0., 1.);
        cover += w;
    }
    [
        0.085 + 0.10 * ember + 2.4 * heat,
        0.065 + 0.70 * heat * heat,
        0.060 + 0.06 * heat,
        cover * 0.95,
    ]
}

/// Source shared-vertex lattice over the touched region, including the one-cell
/// feather margin. Domain-warped samples reconstruct the display perimeter;
/// they never enter the model or the operational web raster.
fn ground_overlay_continuous(
    s: &scenario::Scenario,
    snap: &Snapshot,
    quality: &crate::quality::Quality,
) -> Mesh {
    let g = s.grid;
    let (mut r0, mut c0, mut r1, mut c1) = (usize::MAX, usize::MAX, 0, 0);
    for (i, &code) in snap.cells.iter().enumerate() {
        if matches!(code, 1 | 2) {
            let (r, c) = (i / g.cols, i % g.cols);
            r0 = r0.min(r);
            c0 = c0.min(c);
            r1 = r1.max(r);
            c1 = c1.max(c);
        }
    }
    if r0 == usize::MAX {
        return empty_mesh();
    }
    r0 = r0.saturating_sub(1);
    c0 = c0.saturating_sub(1);
    r1 = (r1 + 2).min(g.rows);
    c1 = (c1 + 2).min(g.cols);
    let span = (r1 - r0).max(c1 - c0);
    let requested = match quality.name {
        "low" => 1,
        "high" => 4,
        _ => 2,
    };
    let subdiv = requested.min((560 / span.max(1)).max(1));
    let step = g.cellsize / subdiv as f32;
    let (nx, ny) = ((c1 - c0) * subdiv + 1, (r1 - r0) * subdiv + 1);
    let field = crate::fire_field::FireField { snap, grid: g };
    let mut positions = Vec::with_capacity(nx * ny);
    let mut colours = Vec::with_capacity(nx * ny);
    for y in 0..ny {
        for x in 0..nx {
            let p = [
                c0 as f32 * g.cellsize + x as f32 * step,
                g.height_m() - r0 as f32 * g.cellsize - y as f32 * step,
            ];
            let sample = field.sample(p);
            let cover = ((sample.coverage - 0.42) / 0.20).clamp(0., 1.);
            let arrival = sample.arrival.unwrap_or(snap.time_s as f32);
            let age = (snap.time_s as f32 - arrival).max(0.);
            let glow = if arrival <= snap.time_s as f32 {
                (1. - age / 1100.).clamp(0., 1.).powi(2)
            } else {
                0.
            };
            let embers = (1. - age / 4400.).clamp(0., 1.);
            let ash = 0.72
                + 0.85
                    * (0.6 * crate::fire_field::noise(p[0] / 23., p[1] / 23., 0x77)
                        + 0.4 * crate::fire_field::noise(p[0] / 8., p[1] / 8., 0x88));
            let col = [
                (0.085 + 0.10 * embers) * ash + 2.4 * glow,
                0.065 * ash + 0.70 * glow * glow,
                0.060 * ash + 0.06 * glow,
                cover * 0.95,
            ];
            let lift = 0.5 + if col[0] > 1. { 1.2 * col[3] } else { 0. };
            positions.push(to_bevy(s, p, lift).to_array());
            colours.push(col);
        }
    }
    let mut remap = vec![u32::MAX; nx * ny];
    let mut out = QuadBuilder::default();
    for y in 0..ny - 1 {
        for x in 0..nx - 1 {
            let q = [
                y * nx + x,
                y * nx + x + 1,
                (y + 1) * nx + x + 1,
                (y + 1) * nx + x,
            ];
            if q.iter().all(|&i| colours[i][3] < 0.02) {
                continue;
            }
            let mut indices = [0; 4];
            for (slot, &i) in indices.iter_mut().zip(&q) {
                if remap[i] == u32::MAX {
                    remap[i] = out.positions.len() as u32;
                    out.positions.push(positions[i]);
                    out.colors.push(colours[i]);
                    out.normals.push([0., 1., 0.]);
                    out.uvs.push([0.5, 0.5]);
                }
                *slot = remap[i];
            }
            out.indices.extend([
                indices[0], indices[3], indices[2], indices[0], indices[2], indices[1],
            ]);
        }
    }
    out.finish()
}

#[cfg(test)]
mod upscale_tests {
    use super::*;
    #[test]
    fn continuous_lattice_shares_vertices_and_rebuilds_from_the_snapshot() {
        let s = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
        let mut snap = Snapshot {
            cells: vec![0; s.grid.len()],
            arrival: vec![i32::MIN; s.grid.len()],
            ..default()
        };
        for r in 49..52 {
            for c in 49..52 {
                let i = r * s.grid.cols + c;
                snap.cells[i] = 1;
                snap.arrival[i] = 0;
            }
        }
        let mut quality = crate::quality::Quality::from_env();
        quality.name = "high";
        let m = ground_overlay_continuous(&s, &snap, &quality);
        let count = m.count_vertices();
        assert!(count > 0);
        let triangles = m.indices().unwrap().len() / 3;
        assert!(
            count < triangles * 2,
            "vertices should be shared between neighbouring quads"
        );
        snap.cells.fill(5); // same arrival values are uncertainty, never confirmed fire
        assert_eq!(
            ground_overlay_continuous(&s, &snap, &quality).count_vertices(),
            0
        );
        snap.cells.fill(3);
        assert_eq!(
            ground_overlay_continuous(&s, &snap, &quality).count_vertices(),
            0
        );
    }
}

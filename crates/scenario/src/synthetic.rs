//! The synthetic "Valverde valley" scenario: a 4 × 4 km window with a forested
//! ridge to the north, a village (Pineta) on the forest edge, a town
//! (Valverde) in the valley and farmland to the south, cut by a river.
//!
//! Deterministic for a given seed. The layout is hand-placed (so injects and
//! the gazetteer can name things); only the fuel mosaic, the terrain texture
//! and the house jitter are random.
//!
//! The opening fire burns on the south-west flank of Pine Hill, ~0.6 km
//! north-east of Pineta, under a 45 km/h north-easterly wind at 4 % fuel
//! moisture: left alone it reaches the village's edge in about an hour, then
//! wraps around it (pinned by `fire/tests/synthetic_spread.rs`). Tuned so that
//! 30-minute turns give the players one turn to act before contact.

use std::collections::BTreeMap;

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::fuel::*;
use crate::*;

pub const ROWS: usize = 200;
pub const COLS: usize = 200;
pub const CELL: f32 = 20.0;
pub const DEFAULT_SEED: u64 = 7;

const VALVERDE: Pos = [1600.0, 1300.0];
const VALVERDE_R: f32 = 380.0;
const PINETA: Pos = [2800.0, 2250.0];
const PINETA_R: f32 = 170.0;
const PINE_HILL: Pos = [3350.0, 3000.0];
/// The opening fire, on the lower south-west flank of Pine Hill.
const IGNITION: Pos = [3150.0, 2720.0];

/// Smooth value noise in [0, 1], `octaves` layers from a `base` wavelength.
struct Noise {
    lattice: Vec<f32>,
    n: usize,
}

impl Noise {
    fn new(rng: &mut ChaCha8Rng) -> Noise {
        let n = 64;
        Noise {
            lattice: (0..n * n).map(|_| rng.gen::<f32>()).collect(),
            n,
        }
    }

    fn sample(&self, x: f32, y: f32) -> f32 {
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
        let n = self.n as i64;
        let at = |i: i64, j: i64| self.lattice[(i.rem_euclid(n) * n + j.rem_euclid(n)) as usize];
        let (i, j) = (x0 as i64, y0 as i64);
        let a = at(i, j) + (at(i + 1, j) - at(i, j)) * sx;
        let b = at(i, j + 1) + (at(i + 1, j + 1) - at(i, j + 1)) * sx;
        a + (b - a) * sy
    }

    fn fbm(&self, p: Pos, base: f32, octaves: u32) -> f32 {
        let (mut amp, mut freq, mut sum, mut norm) = (1.0, 1.0 / base, 0.0, 0.0);
        for _ in 0..octaves {
            sum += amp * self.sample(p[0] * freq, p[1] * freq);
            norm += amp;
            amp *= 0.5;
            freq *= 2.0;
        }
        sum / norm
    }
}

fn dist(a: Pos, b: Pos) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn river_y(x: f32) -> f32 {
    520.0 + 80.0 * (x / 600.0).sin()
}

pub fn generate(seed: u64) -> Scenario {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let terrain_noise = Noise::new(&mut rng);
    let fuel_noise = Noise::new(&mut rng);
    let grid = GridSpec {
        rows: ROWS,
        cols: COLS,
        cellsize: CELL,
    };

    let mut dem = vec![0.0f32; grid.len()];
    let mut fuel = vec![GRASSLAND; grid.len()];
    for row in 0..ROWS {
        for col in 0..COLS {
            let p = [
                (col as f32 + 0.5) * CELL,
                ((ROWS - 1 - row) as f32 + 0.5) * CELL,
            ];
            let i = row * COLS + col;

            // Terrain: valley floor rising to a ridge in the north, Pine Hill on top.
            let ridge = 420.0 * smoothstep(2100.0, 3800.0, p[1]);
            let hill = 140.0 * (-(dist(p, PINE_HILL) / 450.0).powi(2)).exp();
            let texture = 60.0 * (terrain_noise.fbm(p, 900.0, 4) - 0.5);
            dem[i] = 60.0 + 0.01 * p[1] + ridge + hill + texture * smoothstep(1500.0, 3000.0, p[1]);

            // Fuel mosaic by band, broken up by noise.
            let n = fuel_noise.fbm(p, 350.0, 3);
            let forest_edge = 2450.0 + 250.0 * (fuel_noise.fbm([p[0], 0.0], 600.0, 2) - 0.5);
            fuel[i] = if p[1] > forest_edge {
                if dem[i] > 330.0 || n > 0.62 {
                    CONIFERS
                } else if n > 0.42 {
                    SHRUBS
                } else {
                    BROADLEAVES
                }
            } else if p[1] > 1750.0 {
                if n > 0.6 {
                    BROADLEAVES
                } else if n > 0.4 {
                    SHRUBS
                } else {
                    GRASSLAND
                }
            } else if n > 0.7 {
                BROADLEAVES
            } else if n > 0.45 {
                AGRO_FORESTRY
            } else {
                GRASSLAND
            };

            // Non-burnable features: river, settlements.
            if (p[1] - river_y(p[0])).abs() < 22.0 {
                fuel[i] = NON_VEGETATED;
            }
            if dist(p, VALVERDE) < VALVERDE_R || dist(p, PINETA) < PINETA_R {
                fuel[i] = NON_VEGETATED;
            }
        }
    }

    let mut buildings = Vec::new();
    fn add(
        v: &mut Vec<Building>,
        kind: BuildingKind,
        name: Option<&str>,
        pos: Pos,
        size: [f32; 2],
        occupants: u32,
        shelter: u32,
    ) {
        let id = v.len() as u32 + 1;
        v.push(Building {
            id,
            visual_kind: None,
            kind,
            name: name.map(str::to_string),
            pos,
            size,
            occupants,
            sector: String::new(),
            shelter_capacity: shelter,
        });
    }

    // Named facilities first, so their ids are stable across seeds.
    add(
        &mut buildings,
        BuildingKind::School,
        Some("Pineta Primary School"),
        [2760.0, 2210.0],
        [40.0, 24.0],
        120,
        0,
    );
    add(
        &mut buildings,
        BuildingKind::FuelStation,
        Some("Pineta Fuel Station"),
        [2470.0, 2020.0],
        [24.0, 16.0],
        4,
        0,
    );
    add(
        &mut buildings,
        BuildingKind::TownHall,
        Some("Valverde Town Hall"),
        [1600.0, 1330.0],
        [30.0, 24.0],
        40,
        0,
    );
    add(
        &mut buildings,
        BuildingKind::Hospital,
        Some("Valverde Hospital"),
        [1440.0, 1160.0],
        [50.0, 30.0],
        90,
        0,
    );
    add(
        &mut buildings,
        BuildingKind::SportsCentre,
        Some("Valverde Sports Centre"),
        [1830.0, 1150.0],
        [50.0, 36.0],
        0,
        400,
    );
    add(
        &mut buildings,
        BuildingKind::Industrial,
        Some("Valverde Timber Mill"),
        [2150.0, 1650.0],
        [60.0, 30.0],
        25,
        0,
    );

    let taken: Vec<Pos> = buildings.iter().map(|b| b.pos).collect();
    let clear_of_facilities = |p: Pos| taken.iter().all(|t| dist(*t, p) > 45.0);
    let jitter = |rng: &mut ChaCha8Rng| [rng.gen_range(-8.0..8.0), rng.gen_range(-8.0..8.0)];

    // Valverde: a jittered grid of houses inside the town disc.
    let mut houses = Vec::new();
    let mut y = VALVERDE[1] - VALVERDE_R;
    while y < VALVERDE[1] + VALVERDE_R {
        let mut x = VALVERDE[0] - VALVERDE_R;
        while x < VALVERDE[0] + VALVERDE_R {
            let j = jitter(&mut rng);
            let p = [x + j[0], y + j[1]];
            if dist(p, VALVERDE) < VALVERDE_R - 30.0
                && clear_of_facilities(p)
                && rng.gen::<f32>() < 0.8
            {
                houses.push(p);
            }
            x += 55.0;
        }
        y += 55.0;
    }
    // Pineta: a tighter ring village on the forest edge.
    let mut y = PINETA[1] - PINETA_R;
    while y < PINETA[1] + PINETA_R {
        let mut x = PINETA[0] - PINETA_R;
        while x < PINETA[0] + PINETA_R {
            let j = jitter(&mut rng);
            let p = [x + j[0], y + j[1]];
            if dist(p, PINETA) < PINETA_R - 15.0 && clear_of_facilities(p) {
                houses.push(p);
            }
            x += 40.0;
        }
        y += 40.0;
    }
    // Scattered farmhouses across the plain.
    while houses.len() < 260 {
        let p = [rng.gen_range(300.0..3800.0), rng.gen_range(700.0..1900.0)];
        if dist(p, VALVERDE) > VALVERDE_R + 80.0 && clear_of_facilities(p) {
            houses.push(p);
        }
    }
    for p in houses {
        let occ = rng.gen_range(1..=5);
        add(
            &mut buildings,
            BuildingKind::House,
            None,
            p,
            [14.0, 12.0],
            occ,
            0,
        );
    }

    let sectors = vec![
        Sector {
            id: "pineta".into(),
            name: "Pineta".into(),
            bounds: [2450.0, 1950.0, 3200.0, 2600.0],
        },
        Sector {
            id: "valverde".into(),
            name: "Valverde".into(),
            bounds: [1100.0, 850.0, 2150.0, 1750.0],
        },
        Sector {
            id: "north_forest".into(),
            name: "North Forest".into(),
            bounds: [0.0, 2600.0, 4000.0, 4000.0],
        },
        Sector {
            id: "east_farmland".into(),
            name: "Eastern Farmland".into(),
            bounds: [2150.0, 0.0, 4000.0, 2600.0],
        },
        Sector {
            id: "west_plain".into(),
            name: "Western Plain".into(),
            bounds: [0.0, 0.0, 4000.0, 4000.0],
        },
    ];

    // Every building sits on a non-burnable cell: houses never burn in the CA
    // (they are threatened by exposure instead, see `sim::buildings`).
    let mut scn = Scenario {
        tactical_policy: Default::default(),
        operations: Default::default(), communication_sites: vec![], fuel_definitions: vec![],
        id: "synthetic_valley".into(),
        name: "Valverde valley (synthetic)".into(),
        description: "Synthetic 4 x 4 km wildland-urban-interface valley: forested ridge, the village of Pineta on the forest edge with its primary school and fuel station, the town of Valverde with hospital and sports centre, farmland and a river. A wildfire is burning on Pine Hill under a north-easterly wind.".into(),
        grid,
        fuel,
        dem,
        render_terrain: None,
        start_clock_s: 13 * 3600,
        weather: WeatherSpec { wind_dir_deg: 45.0, wind_speed_kmh: 45.0, moisture_pct: 4.0 },
        initial_ignitions: vec![IgnitionSpec { at: IGNITION, radius_m: 120.0 }],
        buildings,
        roads: roads(),
        sectors,
        zones: ZoneGrid { cols: 8, rows: 8, size_m: 500.0 },
        places: places(),
        resource_types: resource_types(),
        roles: roles(),
        reinforcements: reinforcements(),
        msel: msel(),
    };
    for i in 0..scn.buildings.len() {
        let p = scn.buildings[i].pos;
        scn.buildings[i].sector = scn
            .sector_at(p)
            .map(|s| s.id.clone())
            .unwrap_or_else(|| "west_plain".into());
        if let Some(c) = scn.cell_of(p) {
            let idx = scn.grid.index(c);
            scn.fuel[idx] = NON_VEGETATED;
        }
    }
    scn
}

fn roads() -> Vec<Road> {
    let road = |id: &str, name: &str, pts: &[Pos]| Road {
        oneway: false,
        class: if id == "ridge_track" {
            "track"
        } else {
            "residential"
        }
        .into(),
        drivable: id != "ridge_track",
        id: id.into(),
        name: name.into(),
        points: pts.to_vec(),
    };
    vec![
        road(
            "sp1",
            "SP1 Main Road",
            &[
                [0.0, 1300.0],
                [1200.0, 1300.0],
                [1600.0, 1300.0],
                [2000.0, 1350.0],
                [3000.0, 1420.0],
                [4000.0, 1500.0],
            ],
        ),
        road(
            "north_road",
            "Northern Road",
            &[
                [1600.0, 1300.0],
                [2000.0, 1600.0],
                [2450.0, 2000.0],
                [2800.0, 2250.0],
                [3100.0, 2700.0],
                [3300.0, 3400.0],
            ],
        ),
        road(
            "ridge_track",
            "Ridge Track",
            &[
                [2000.0, 3300.0],
                [2800.0, 3150.0],
                [3600.0, 3300.0],
                [4000.0, 3250.0],
            ],
        ),
        road(
            "south_road",
            "Southern Road",
            &[[1600.0, 1300.0], [1500.0, 800.0], [1400.0, 0.0]],
        ),
    ]
}

fn places() -> Vec<Place> {
    let place = |name: &str, aliases: &[&str], pos: Pos| Place {
        name: name.into(),
        aliases: aliases.iter().map(|s| s.to_string()).collect(),
        pos,
    };
    vec![
        place("Pine Hill", &["Monte Pino", "Collina dei Pini"], PINE_HILL),
        place(
            "North Ridge",
            &["Cresta Nord", "the ridge"],
            [2800.0, 3500.0],
        ),
        place("Pineta", &["Pineta village", "the village"], PINETA),
        place("Valverde", &["Valverde town", "the town"], VALVERDE),
        place(
            "Pineta north edge",
            &["north of Pineta", "margine nord di Pineta"],
            [2800.0, 2480.0],
        ),
        place(
            "Pineta east edge",
            &["east of Pineta", "margine est di Pineta"],
            [3030.0, 2250.0],
        ),
        place("Pineta west edge", &["west of Pineta"], [2570.0, 2300.0]),
        place(
            "Northern Road junction",
            &["junction", "incrocio"],
            [2450.0, 2000.0],
        ),
        place(
            "Forest edge east",
            &["eastern forest edge"],
            [3400.0, 2500.0],
        ),
        place(
            "Forest edge west",
            &["western forest edge"],
            [2200.0, 2500.0],
        ),
        place(
            "River Lenta",
            &["the river", "fiume Lenta"],
            [2000.0, 520.0],
        ),
    ]
}

fn resource_types() -> Vec<ResourceType> {
    let rt = |id: &str, name: &str, d: &str| ResourceType {
        id: id.into(),
        name: name.into(),
        description: d.into(),
    };
    vec![
        rt(
            "engines",
            "Road engines",
            "2,500 L tank; 60 m hose reach; refills at the staging base.",
        ),
        rt(
            "ground_crews",
            "Ground crews",
            "Each crew can cut up to 600 m of fire line (fuel break) per turn.",
        ),
        rt(
            "air_tankers",
            "Air tankers",
            "Each air tanker makes one water drop (about 60 m radius) per turn.",
        ),
        rt(
            "buses",
            "Buses",
            "50 seats each; needed to evacuate schools and hospitals. Committed for the turn.",
        ),
        rt(
            "police_patrols",
            "Police patrols",
            "Each patrol can close one road to civilian traffic per turn.",
        ),
    ]
}

fn roles() -> Vec<Role> {
    let res = |pairs: &[(&str, u32)]| {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), *v))
            .collect::<BTreeMap<_, _>>()
    };
    let acts = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    vec![
        Role {
            id: "mayor".into(),
            name: "Mayor of Valverde".into(),
            level: "local".into(),
            language: "it".into(),
            description: "Local authority for Valverde and Pineta: warnings, evacuation orders, shelters, municipal police.".into(),
            allowed_actions: acts(&["EvacuateZone", "EvacuateBuilding", "IssueWarning", "OpenShelter", "CloseRoad", "ReopenRoad"]),
            resources: res(&[("buses", 2), ("police_patrols", 2)]),
        },
        Role {
            id: "fire_commander".into(),
            name: "Fire and Rescue Commander".into(),
            level: "local".into(),
            language: "en".into(),
            description: "Commands firefighting: fire lines (fuel breaks) with ground crews, water drops with air tankers.".into(),
            allowed_actions: acts(&["CutFireLine", "WaterDrop", "DeployEngine"]),
            resources: res(&[("ground_crews", 3), ("air_tankers", 1), ("engines", 2)]),
        },
        Role {
            id: "regional_cp".into(),
            name: "Regional Civil Protection".into(),
            level: "regional".into(),
            language: "en".into(),
            description: "Regional coordination: requests reinforcements (regional and international/UCPM), opens shelters, can order evacuations and warnings.".into(),
            allowed_actions: acts(&["RequestReinforcement", "OpenShelter", "IssueWarning", "EvacuateZone", "EvacuateBuilding"]),
            resources: res(&[]),
        },
    ]
}

fn reinforcements() -> Vec<ReinforcementOffer> {
    let r = |resource: &str, max_qty, delay_min, source: &str, to: &str| ReinforcementOffer {
        resource: resource.into(),
        max_qty,
        delay_min,
        source: source.into(),
        assigned_to: to.into(),
    };
    vec![
        r(
            "ground_crews",
            3,
            60,
            "Regional fire service",
            "fire_commander",
        ),
        r(
            "air_tankers",
            2,
            120,
            "International assistance (UCPM / rescEU)",
            "fire_commander",
        ),
        r("buses", 4, 30, "Regional transport company", "mayor"),
        r("police_patrols", 2, 30, "Regional police", "mayor"),
    ]
}

fn msel() -> Vec<InjectTemplate> {
    let inj =
        |id: &str, turn, title: &str, sender: &str, to: &[&str], text: &str, objective: &str| {
            InjectTemplate {
                id: id.into(),
                turn,
                title: title.into(),
                sender: sender.into(),
                recipients: to.iter().map(|s| s.to_string()).collect(),
                text: text.into(),
                objective: objective.into(),
            }
        };
    vec![
        inj("I-01", 1, "Fire reported on Pine Hill", "112 Dispatch", &["all"],
            "13:00 – Multiple calls report a wildfire on Pine Hill, in the North Forest about 600 m north-east of Pineta. Wind from the north-east at 45 km/h, very dry vegetation. The fire is estimated at 5 hectares and spreading south-west.",
            "Initial situational awareness and information sharing"),
        inj("I-02", 1, "School in session", "Pineta Primary School", &["mayor"],
            "The Pineta Primary School is in session: 120 pupils and staff are on site. The head teacher asks whether to keep the children inside or prepare to leave. The school has no transport of its own.",
            "Early action for vulnerable groups; evacuation of a critical facility"),
        inj("I-03", 1, "Fire behaviour estimate", "Fire operations room", &["fire_commander"],
            "Fire modelling indicates that, without intervention, the head of the fire may reach the northern edge of Pineta within about 90 minutes. You have 3 ground crews and 1 air tanker available.",
            "Tactical use of fire lines and aerial resources"),
        inj("I-04", 2, "Wind strengthening", "Regional weather service", &["all"],
            "Updated forecast: north-east wind strengthening to 55 km/h in the next hours, relative humidity dropping. Spotting ahead of the front is likely.",
            "Interpreting forecast information; anticipating escalation"),
        inj("I-05", 2, "Northern Road congestion", "Municipal police", &["mayor", "fire_commander"],
            "Residents leaving Pineta by car are congesting the Northern Road near the junction. Fire engines report delays reaching the village.",
            "Traffic management; competing use of the evacuation route"),
        inj("I-06", 3, "Fuel station threatened", "Fire operations room", &["fire_commander", "mayor"],
            "The fire front is approaching the Pineta Fuel Station. The station holds about 20,000 litres of fuel. An explosion could damage the power line along the Northern Road.",
            "Protection of critical infrastructure; cascading effects"),
        inj("I-07", 3, "ERCC monitoring", "ERCC (EU Emergency Response Coordination Centre)", &["regional_cp"],
            "The ERCC is monitoring the severe wildfire situation in your region. Other countries are also affected. Please provide an updated situation report and your anticipated needs, should you consider requesting assistance through the Union Civil Protection Mechanism.",
            "International assistance / UCPM interface"),
        inj("I-08", 4, "Local capacity exhausted", "Fire operations room", &["regional_cp", "fire_commander"],
            "All local ground crews are committed and exhausted. A second fire front is developing on the western forest edge. Local capacity is no longer sufficient.",
            "Escalation decision under capacity stress"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let a = generate(DEFAULT_SEED);
        let b = generate(DEFAULT_SEED);
        assert_eq!(a.fuel, b.fuel);
        assert_eq!(a.buildings.len(), b.buildings.len());
        assert_eq!(a.buildings[40].pos, b.buildings[40].pos);
    }

    #[test]
    fn consistent_and_complete() {
        let s = generate(DEFAULT_SEED);
        s.check().unwrap();
        assert!(s.buildings.len() >= 260, "{} buildings", s.buildings.len());
        assert!(s
            .buildings
            .iter()
            .any(|b| b.kind == BuildingKind::School && b.sector == "pineta"));
        assert!(s.buildings.iter().any(|b| b.shelter_capacity > 0));
        assert_eq!(s.roles.len(), 3);
    }

    #[test]
    fn buildings_sit_on_non_burnable_cells() {
        let s = generate(DEFAULT_SEED);
        for b in &s.buildings {
            let c = s.cell_of(b.pos).unwrap();
            assert!(!s.is_burnable(c), "building {} on burnable fuel", b.id);
        }
    }

    #[test]
    fn ignition_is_in_the_forest() {
        let s = generate(DEFAULT_SEED);
        let c = s.cell_of(s.initial_ignitions[0].at).unwrap();
        assert!(s.is_burnable(c));
    }

    #[test]
    fn gazetteer_resolves_aliases_facilities_and_sectors() {
        let s = generate(DEFAULT_SEED);
        assert_eq!(s.resolve_place("monte pino"), Some(PINE_HILL));
        assert!(s.resolve_place("Pineta Primary School").is_some());
        assert!(s.resolve_place("pineta").is_some());
        assert_eq!(s.resolve_place("F4"), s.zone_centre("F4"));
        assert!(s.resolve_place("Northern Road").is_some());
        assert!(s.resolve_place("Atlantis").is_none());
    }

    #[test]
    fn school_is_in_zone_f4() {
        let s = generate(DEFAULT_SEED);
        let school = s.resolve_place("Pineta Primary School").unwrap();
        assert_eq!(s.zone_of(school).as_deref(), Some("F4"));
    }

    #[test]
    fn cell_round_trip() {
        let s = generate(DEFAULT_SEED);
        let c = Cell { row: 10, col: 150 };
        assert_eq!(s.cell_of(s.cell_centre(c)), Some(c));
        // Row 0 is the north edge.
        assert!(
            s.cell_centre(Cell { row: 0, col: 0 })[1] > s.cell_centre(Cell { row: 199, col: 0 })[1]
        );
    }
}

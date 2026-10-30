//! What the world looks like from outside: a compact **digest** for LLM
//! prompts (role-filtered), and a **view** for the web UIs (fire raster
//! included).

use base64::Engine;
use fire::CellFire;
use serde::Serialize;
use serde_json::{json, Value};

use crate::{is_facility, Damage, World};

/// Per-cell code in the view's fire raster.
pub mod cell_code {
    pub const UNBURNT: u8 = 0;
    pub const BURNING: u8 = 1;
    pub const BURNT: u8 = 2;
    pub const FIRE_LINE: u8 = 3;
    pub const WET: u8 = 4;
    /// Unburnt, but reached by some realizations (below the cutoff): the uncertainty halo.
    pub const POSSIBLE: u8 = 5;
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildingView {
    pub id: u32,
    /// 0 intact, 1 damaged, 2 destroyed
    pub damage: u8,
    pub threatened: bool,
    pub inside: u32,
    pub evacuating: bool,
    pub evacuated: u32,
}

impl World {
    /// Compact JSON describing the situation, for an LLM prompt. `role`
    /// restricts resources to that role; `None` is the EXCON's full view.
    pub fn digest(&self, role: Option<&str>) -> Value {
        let s = self.stats();
        let w = self.fire.weather();
        let facilities: Vec<Value> = self
            .scn
            .buildings
            .iter()
            .zip(&self.d.buildings)
            .filter(|(b, _)| is_facility(b.kind))
            .map(|(b, st)| {
                json!({
                    "name": b.name, "kind": b.kind, "zone": self.scn.zone_of(b.pos),
                    "people_inside": st.people_inside, "evacuated": st.evacuated,
                    "evacuation_in_progress": st.evac_done_at.is_some(),
                    "damage": st.damage, "fire_within_300m": st.threatened,
                    "shelter_capacity": b.shelter_capacity,
                    "shelter_open": self.d.shelters.contains_key(&b.id),
                })
            })
            .collect();
        // Houses aggregated by zone.
        let mut zones: std::collections::BTreeMap<String, [u32; 5]> = Default::default();
        for (b, st) in self.scn.buildings.iter().zip(&self.d.buildings) {
            let Some(z) = self.scn.zone_of(b.pos) else {
                continue;
            };
            let e = zones.entry(z).or_default();
            e[0] += 1;
            e[1] += st.people_inside;
            e[2] += st.evacuated;
            e[3] += (st.damage != Damage::Intact) as u32;
            e[4] += st.threatened as u32;
        }
        let populated: Vec<Value> = zones
            .iter()
            .map(|(z, v)| json!({"zone": z, "buildings": v[0], "people_inside": v[1], "evacuated": v[2], "damaged_or_destroyed": v[3], "threatened": v[4], "warned": self.d.warned.contains(z)}))
            .collect();
        let roads: Vec<Value> = self
            .scn
            .roads
            .iter()
            .map(|r| {
                let st = &self.d.roads[&r.id];
                json!({"id": r.id, "name": r.name, "closed": st.closed, "cut_by_fire": st.cut_by_fire, "fire_closed_links": st.fire_closed_links})
            })
            .collect();
        let pools: Value = match role {
            Some(r) => json!({ r: self.d.pools.get(r) }),
            None => json!(self.d.pools),
        };
        let arrivals: Vec<Value> = self
            .d
            .arrivals
            .iter()
            .filter(|a| a.at_s > self.time_s() && role.is_none_or(|r| r == a.role))
            .map(|a| json!({"resource": a.resource, "qty": a.qty, "for": a.role, "eta": scenario::format_clock(self.scn.start_clock_s + a.at_s), "from": a.source}))
            .collect();
        json!({
            "clock": s.clock,
            "weather": {"wind_from_deg": w.wind_dir_deg, "wind_kmh": w.wind_speed_kmh, "fuel_moisture_pct": w.moisture_pct},
            "fire": {"burned_ha": (s.burned_ha * 10.0).round() / 10.0, "burning_zones": self.burning_zones(), "fire_lines_km": s.line_km, "water_drops": s.drops,
                     "model": format!("{}-run Monte Carlo ensemble; the fire is the set of cells reached by at least {:.0}% of the runs", self.fire.options().realizations, self.fire.options().cutoff * 100.0)},
            "totals": {"buildings_damaged": s.damaged, "buildings_destroyed": s.destroyed, "buildings_threatened": s.threatened, "people_evacuated": s.evacuated_people, "people_at_risk": s.people_at_risk, "people_sheltered": s.sheltered},
            "facilities": facilities,
            "populated_zones": populated,
            "roads": roads,
            "resources": pools,
            "resources_this_turn": pools,
            "reinforcements_en_route": arrivals,
            "open_shelters": self.d.shelters.keys().filter_map(|id| self.scn.buildings.iter().find(|b| b.id == *id).and_then(|b| b.name.clone())).collect::<Vec<_>>(),
        })
    }

    /// Place names, zone layout and resource meanings: the static context an
    /// LLM needs to turn words into commands.
    pub fn gazetteer(&self) -> Value {
        let z = self.scn.zones;
        json!({
            "map": format!("{:.0} x {:.0} m. Evacuation-zone grid {}x{} of {:.0} m squares: columns {}..{} west to east, rows 1..{} north to south (A1 = north-west corner). Zone edges: \"F3:N\" / \"F3:S\" / \"F3:E\" / \"F3:W\".",
                self.scn.grid.width_m(), self.scn.grid.height_m(), z.cols, z.rows, z.size_m, 'A', (b'A' + z.cols as u8 - 1) as char, z.rows),
            "places": self.scn.places.iter().map(|p| json!({"name": p.name, "aliases": p.aliases, "zone": self.scn.zone_of(p.pos)})).collect::<Vec<_>>(),
            "facilities": self.scn.buildings.iter().filter(|b| b.name.is_some()).map(|b| json!({"name": b.name, "kind": b.kind, "zone": self.scn.zone_of(b.pos)})).collect::<Vec<_>>(),
            "roads": self.scn.roads.iter().map(|r| json!({"id": r.id, "name": r.name, "zones": r.points.iter().filter_map(|p| self.scn.zone_of(*p)).collect::<Vec<_>>()})).collect::<Vec<_>>(),
            "resources": self.scn.resource_types.iter().map(|r| json!({"id": r.id, "meaning": r.description})).collect::<Vec<_>>(),
            "reinforcement_offers": self.scn.reinforcements.iter().map(|o| json!({"resource": o.resource, "max_qty": o.max_qty, "delay_min": o.delay_min, "source": o.source, "assigned_to": o.assigned_to})).collect::<Vec<_>>(),
        })
    }

    /// Fire raster for the UIs: base64 of one [`cell_code`] byte per cell, row 0 north.
    pub fn fire_raster_b64(&self) -> String {
        let cleared = self.fire.cleared();
        let wet = self.fire.added_moisture();
        let prob = self.fire.probability();
        let bytes: Vec<u8> = self
            .fire
            .state()
            .iter()
            .enumerate()
            .map(|(i, s)| match s {
                CellFire::Burning => cell_code::BURNING,
                CellFire::Burnt => cell_code::BURNT,
                CellFire::Unburnt if cleared[i] => cell_code::FIRE_LINE,
                CellFire::Unburnt if wet[i] > 10.0 => cell_code::WET,
                CellFire::Unburnt if prob[i] > 0.0 => cell_code::POSSIBLE,
                CellFire::Unburnt => cell_code::UNBURNT,
            })
            .collect();
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    /// Per-building state as the web maps draw it.
    pub fn building_views(&self) -> Vec<BuildingView> {
        self.d
            .buildings
            .iter()
            .map(|b| BuildingView {
                id: b.id,
                damage: match b.damage {
                    Damage::Intact => 0,
                    Damage::Damaged => 1,
                    Damage::Destroyed => 2,
                },
                threatened: b.threatened,
                inside: b.people_inside,
                evacuating: b.evac_done_at.is_some(),
                evacuated: b.evacuated,
            })
            .collect()
    }

    /// Dynamic state for the web UIs.
    pub fn view(&self) -> Value {
        let buildings = self.building_views();
        json!({
            "operations":self.scn.operations,
            "tactical_policy":self.scn.tactical_policy,
            "traffic":self.traffic_view(),
            "deployments":self.deployment_view(),
            "travel_settings":self.d.travel_settings,
            "stats": self.stats(),
            "weather": self.fire.weather(),
            "ensemble": self.fire.options(),
            "fire": self.fire_raster_b64(),
            "buildings": buildings,
            "roads": self.d.roads,
            "lines": self.d.lines,
            "drops": self.d.drops,
            "warned": self.d.warned,
            "shelters": self.d.shelters,
            "pools": self.d.pools,
            "allocations": self.d.allocations,
            "resource_events": self.d.resource_events,
            "arrivals": self.d.arrivals,
            "burning_zones": self.burning_zones(),
        })
    }

    /// Static scenario data for the web UIs (fuel raster included, no DEM).
    pub fn static_view(&self) -> Value {
        let fuel: Vec<u8> = self.scn.fuel.iter().map(|&f| f as u8).collect();
        // Keyed like the raster bytes, so the UIs can name every class they see.
        let mut ids: Vec<i32> = self.scn.fuel.clone();
        ids.sort_unstable();
        ids.dedup();
        let fuel_names: serde_json::Map<String, Value> = ids
            .into_iter()
            .map(|id| ((id as u8).to_string(), json!(self.scn.fuel_name(id))))
            .collect();
        json!({
            "id": self.scn.id,
            "name": self.scn.name,
            "description": self.scn.description,
            "grid": self.scn.grid,
            "zones": self.scn.zones,
            "fuel": base64::engine::general_purpose::STANDARD.encode(fuel),
            "fuel_names": fuel_names,
            "buildings": self.scn.buildings,
            "roads": self.scn.roads,
            "road_segments": self.network.segments(),
            "places": self.scn.places,
            "roles": self.scn.roles,
            "resource_types": self.scn.resource_types,
            "reinforcements": self.scn.reinforcements,
            "start_clock_s": self.scn.start_clock_s,
        })
    }
}

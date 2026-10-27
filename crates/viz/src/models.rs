//! Embedded Blender bakes from propagator_abm: vegetation varieties, town kit
//! and articulated PBR units. Keep source surface identifiers and rig pivots.

use std::collections::HashMap;
use std::sync::OnceLock;

pub struct Model {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

/// Live plant and fire additions share the same authored origin and structure.
/// Stems stay in place; foliage collapses and fine branches emerge with burn age.
pub struct PlantVariety {
    pub live: Model,
    pub wood: Vec<bool>,
    pub remains: Model,
    pub debris: Vec<bool>,
}

pub fn plant(species: &str, selection: f32) -> &'static PlantVariety {
    static PLANTS: OnceLock<HashMap<String, PlantVariety>> = OnceLock::new();
    let species = match species {
        "oak" => "chestnut",
        other => other,
    };
    let count = if species == "olive" { 3 } else { 4 };
    let name = format!(
        "{species}_{}",
        (selection.clamp(0.0, 0.99999) * count as f32) as usize + 1
    );
    &PLANTS.get_or_init(|| {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../../../assets/models/vegetation.json"))
                .expect("valid vegetation bake");
        let parse = |v: &serde_json::Value| Model {
            positions: serde_json::from_value(v["positions"].clone()).unwrap(),
            normals: serde_json::from_value(v["normals"].clone()).unwrap(),
            colors: serde_json::from_value::<Vec<Vec<f32>>>(v["colors"].clone())
                .unwrap()
                .into_iter()
                .map(|c| [c[0], c[1], c[2], c.get(3).copied().unwrap_or(1.0)])
                .collect(),
            indices: serde_json::from_value(v["indices"].clone()).unwrap(),
        };
        v.as_object()
            .unwrap()
            .iter()
            .map(|(name, v)| {
                (
                    name.clone(),
                    PlantVariety {
                        live: parse(&v["live"]),
                        wood: serde_json::from_value(v["live"]["wood"].clone()).unwrap(),
                        remains: parse(&v["remains"]),
                        debris: serde_json::from_value(v["remains"]["debris"].clone()).unwrap(),
                    },
                )
            })
            .collect()
    })[&name]
}

#[cfg(test)]
pub fn model(name: &str) -> &'static Model {
    static MODELS: OnceLock<HashMap<String, Model>> = OnceLock::new();
    &MODELS.get_or_init(|| {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../../../assets/models/meshes.json"))
                .expect("valid mesh bake");
        v.as_object()
            .unwrap()
            .iter()
            .map(|(name, v)| {
                (
                    name.clone(),
                    Model {
                        positions: serde_json::from_value(v["positions"].clone()).unwrap(),
                        normals: serde_json::from_value(v["normals"].clone()).unwrap(),
                        colors: serde_json::from_value(v["colors"].clone()).unwrap(),
                        indices: serde_json::from_value(v["indices"].clone()).unwrap(),
                    },
                )
            })
            .collect()
    })[name]
}

#[cfg(test)]
mod tests {
    #[test]
    fn vegetation_models_load() {
        for n in ["pine", "oak", "bush"] {
            let m = super::model(n);
            assert!(!m.positions.is_empty());
            assert_eq!(m.positions.len(), m.colors.len());
            assert_eq!(m.indices.len() % 3, 0);
        }
    }

    #[test]
    fn all_fifteen_plant_varieties_have_consistent_live_and_burnt_geometry() {
        for species in ["pine", "oak", "bush", "olive"] {
            let count = if species == "olive" { 3 } else { 4 };
            for i in 0..count {
                let p = super::plant(species, i as f32 / count as f32);
                assert_eq!(p.wood.len(), p.live.positions.len());
                if species != "bush" {
                    assert!(p.wood.iter().any(|w| *w));
                }
                for m in [&p.live, &p.remains] {
                    assert!(!m.positions.is_empty());
                    assert_eq!(m.positions.len(), m.colors.len());
                    assert_eq!(m.positions.len(), m.normals.len());
                    assert!(m.indices.iter().all(|j| (*j as usize) < m.positions.len()));
                }
            }
        }
    }
}

/// Additional landmark and vehicle kit, baked in metres from propagator_abm.
pub fn urban_model(name: &str) -> Option<&'static Model> {
    static URBAN: OnceLock<HashMap<String, Model>> = OnceLock::new();
    URBAN
        .get_or_init(|| {
            let v: serde_json::Value =
                serde_json::from_str(include_str!("../../../assets/models/town.json"))
                    .expect("valid urban mesh bake");
            v.as_object()
                .unwrap()
                .iter()
                .map(|(name, v)| {
                    let colors: Vec<Vec<f32>> =
                        serde_json::from_value(v["colors"].clone()).unwrap();
                    (
                        name.clone(),
                        Model {
                            positions: serde_json::from_value(v["positions"].clone()).unwrap(),
                            normals: serde_json::from_value(v["normals"].clone()).unwrap(),
                            colors: colors
                                .into_iter()
                                .map(|c| [c[0], c[1], c[2], c.get(3).copied().unwrap_or(1.0)])
                                .collect(),
                            indices: serde_json::from_value(v["indices"].clone()).unwrap(),
                        },
                    )
                })
                .collect()
        })
        .get(name)
}

#[cfg(test)]
mod urban_tests {
    #[test]
    fn landmarks_have_valid_meshes_and_status_color_attributes() {
        for name in ["school", "townhall", "fuel", "industrial"] {
            let m = super::urban_model(name).unwrap();
            assert!(!m.positions.is_empty());
            assert_eq!(m.positions.len(), m.colors.len());
            assert_eq!(m.positions.len(), m.normals.len());
            assert!(m.indices.iter().all(|i| (*i as usize) < m.positions.len()));
        }
    }
}

// Latest unit bake: preserve the surface identifier in vertex alpha. Parts
// rotate about authored pivots, never about their bounding-box centres.
use bevy::prelude::*;
use bevy::render::{
    mesh::{Indices, PrimitiveTopology},
    render_asset::RenderAssetUsages,
};
#[derive(Clone, Debug)]
pub struct Rig {
    pub pivot: Vec3,
    pub axis: Vec3,
    pub kind: String,
    pub phase: f32,
}
pub struct Unit {
    pub model: Model,
    pub rig: Option<Rig>,
}
pub fn units() -> &'static HashMap<String, Unit> {
    static UNITS: OnceLock<HashMap<String, Unit>> = OnceLock::new();
    UNITS.get_or_init(|| {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../../../assets/models/units.json"))
                .expect("valid unit bake");
        v.as_object()
            .unwrap()
            .iter()
            .map(|(name, v)| {
                let colors: Vec<[f32; 3]> = serde_json::from_value(v["colors"].clone()).unwrap();
                let surfaces: Vec<u8> = serde_json::from_value(v["surfaces"].clone()).unwrap();
                assert_eq!(colors.len(), surfaces.len());
                let model = Model {
                    positions: serde_json::from_value(v["positions"].clone()).unwrap(),
                    normals: serde_json::from_value(v["normals"].clone()).unwrap(),
                    colors: colors
                        .iter()
                        .zip(surfaces)
                        .map(|(c, s)| [c[0], c[1], c[2], crate::materials::surface_alpha(s)])
                        .collect(),
                    indices: serde_json::from_value(v["indices"].clone()).unwrap(),
                };
                let rig = v.get("rig").map(|r| Rig {
                    pivot: Vec3::from_array(serde_json::from_value(r["pivot"].clone()).unwrap()),
                    axis: Vec3::from_array(serde_json::from_value(r["axis"].clone()).unwrap()),
                    kind: r["kind"].as_str().unwrap().into(),
                    phase: r["phase"].as_f64().unwrap_or(0.0) as f32,
                });
                (name.clone(), Unit { model, rig })
            })
            .collect()
    })
}
pub fn mesh(m: &Model, shift: Vec3) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        m.positions
            .iter()
            .map(|p| (Vec3::from_array(*p) - shift).to_array())
            .collect::<Vec<_>>(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, m.normals.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, m.colors.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0., 0.]; m.positions.len()]);
    mesh.insert_indices(Indices::U32(m.indices.clone()));
    mesh
}
pub fn unit_parts(name: &str) -> Vec<(Mesh, Rig)> {
    let prefix = format!("{name}.");
    let mut parts: Vec<_> = units()
        .iter()
        .filter(|(key, _)| key.starts_with(&prefix))
        .collect();
    parts.sort_by_key(|(key, _)| *key);
    parts
        .into_iter()
        .filter_map(|(_, u)| u.rig.clone().map(|rig| (mesh(&u.model, rig.pivot), rig)))
        .collect()
}
#[cfg(test)]
mod pbr_tests {
    #[test]
    fn articulated_units_preserve_pivots_and_surface_ids() {
        for name in ["engine", "tanker", "firefighter"] {
            let u = &super::units()[name];
            assert!(u.model.positions.len() > 1000);
            assert!(u.model.colors.iter().any(|c| c[3] < 0.1));
            assert!(u
                .model
                .indices
                .iter()
                .all(|i| (*i as usize) < u.model.positions.len()));
            let parts = super::unit_parts(name);
            assert!(!parts.is_empty());
            for (_, r) in parts {
                assert!(r.pivot.is_finite());
                assert!(r.axis.length() > 0.9);
            }
        }
    }
}

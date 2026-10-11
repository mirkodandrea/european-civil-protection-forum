//! Blender-authored meshes from `propagator_abm` (`assets/models/meshes.json`):
//! positions, normals, vertex colours and triangle indices, Y up, ~1 m tall.

use std::collections::HashMap;
use std::sync::OnceLock;

pub struct Model {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

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

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
        let v: serde_json::Value = serde_json::from_str(include_str!("../../../assets/models/meshes.json")).expect("valid mesh bake");
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

use scenario::{editor::Document, Scenario};
#[test]
fn shipped_territories_can_be_edited_and_cloned_without_losing_exercise_content() {
    for id in ["valverde_factory", "selvariva_basin"] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../data/scenarios/{id}.json"));
        let s = Scenario::load(path).unwrap();
        let mut d = Document::new(s.clone()).unwrap();
        d.clone_as(&format!("{id}-copy")).unwrap();
        assert_eq!(d.scene.msel.len(), s.msel.len());
        assert_eq!(
            serde_json::to_value(&d.scene.roles).unwrap(),
            serde_json::to_value(&s.roles).unwrap()
        );
        d.brush([100., 100.], 60., Some(80.), None).unwrap();
        let text = d.export().unwrap();
        let mut reopened = Document::new(s).unwrap();
        reopened.import(&text).unwrap();
        assert_eq!(text, reopened.export().unwrap());
    }
}

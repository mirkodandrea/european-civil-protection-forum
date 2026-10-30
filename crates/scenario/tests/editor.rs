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

#[test]
fn roads_can_be_extended_into_polylines() {
    use scenario::editor::{blank, Selection};
    let mut d = Document::new(blank("poly", 16, 20.).unwrap()).unwrap();
    let Selection::Road(i) = d.add("road", [20., 20.], Some([100., 20.])).unwrap() else {
        panic!("road expected")
    };
    d.extend_road(i, [100., 150.]).unwrap();
    assert_eq!(d.scene.roads[i].points.len(), 3);
    assert!(d.extend_road(i, [100., 150.]).is_err());
    assert!(d.extend_road(i, [1e6, 0.]).is_err());
    assert_eq!(d.scene.roads[i].points.len(), 3);
    d.undo();
    assert_eq!(d.scene.roads[i].points.len(), 2);
}

#[test]
fn sculpt_raises_and_lowers_with_falloff_and_a_stroke_is_one_undo_step() {
    use scenario::editor::blank;
    let mut d = Document::new(blank("sculpt", 16, 20.).unwrap()).unwrap();
    let centre = [160., 160.];
    let cell = |d: &Document, p: [f32; 2]| {
        let c = d.scene.cell_of(p).unwrap();
        d.scene.dem[c.row * d.scene.grid.cols + c.col]
    };
    let flat = cell(&d, centre);
    d.begin_stroke();
    for _ in 0..5 {
        d.sculpt(centre, 100., 2.).unwrap();
    }
    d.end_stroke();
    let raised = cell(&d, centre);
    assert!(raised > flat + 5., "centre rises: {flat} -> {raised}");
    assert!(cell(&d, [160., 250.]) < raised, "falloff towards the edge");
    d.sculpt(centre, 100., -4.).unwrap();
    assert!(cell(&d, centre) < raised);
    d.undo();
    d.undo();
    assert_eq!(cell(&d, centre), flat, "stroke undone in one step");
    assert!(d.sculpt(centre, 0., 1.).is_err());
}

#[test]
fn buildings_have_variants_rotation_and_places_reference_them() {
    use scenario::editor::{blank, BuildingSpec, Selection};
    use scenario::BuildingKind;
    let mut d = Document::new(blank("variants", 16, 20.).unwrap()).unwrap();
    let Selection::Building(i) = d
        .add_building(
            [100., 100.],
            BuildingSpec {
                kind: BuildingKind::Sensitive,
                visual_kind: Some("church".into()),
                size: [30., 12.],
                rotation_deg: -45.,
                occupants: 40,
            },
        )
        .unwrap()
    else {
        panic!("building expected")
    };
    let b = d.scene.buildings[i].clone();
    assert_eq!((b.kind, b.visual_kind.as_deref(), b.rotation_deg), (BuildingKind::Sensitive, Some("church"), 315.));
    let json = d.export().unwrap();
    let mut again = Document::new(d.scene.as_ref().clone()).unwrap();
    again.import(&json).unwrap();
    assert_eq!(again.scene.buildings[i].rotation_deg, 315.);
    assert!(d.add_building([100., 100.], BuildingSpec { size: [1., 1.], ..Default::default() }).is_err());

    let Selection::Place(p) = d.add("place", [120., 120.], None).unwrap() else {
        panic!("place expected")
    };
    d.edit(|s| {
        s.places[p].radius_m = Some(120.);
        s.places[p].buildings = vec![b.id];
        Ok(())
    })
    .unwrap();
    assert!(d.edit(|s| { s.places[p].buildings.push(9999); Ok(()) }).is_err());
    assert!(d.edit(|s| { s.places[p].radius_m = Some(0.); Ok(()) }).is_err());
    d.delete(Selection::Building(i)).unwrap();
    assert!(d.scene.places[p].buildings.is_empty(), "deleting a building drops the reference");
}

#[test]
fn ground_below_sea_level_floods_and_cannot_be_repainted_or_built_on() {
    use scenario::editor::{blank, underwater, BuildingSpec};
    let mut d = Document::new(blank("lake", 16, 20.).unwrap()).unwrap();
    let at = [160., 160.];
    d.brush(at, 60., None, Some(5)).unwrap();
    d.brush(at, 60., Some(-8.), None).unwrap();
    let c = d.scene.cell_of(at).unwrap();
    assert!(underwater(&d.scene, at));
    assert!(!d.scene.is_burnable(c), "lake cells do not burn");
    // Painting vegetation on the lake bed is inert; dry ground stays paintable.
    d.brush(at, 60., None, Some(5)).unwrap();
    assert!(!d.scene.is_burnable(c));
    d.brush([40., 40.], 20., None, Some(5)).unwrap();
    assert!(d.scene.is_burnable(d.scene.cell_of([40., 40.]).unwrap()));
    assert!(d.add_building(at, BuildingSpec::default()).is_err());
    // Raising the bed above sea level makes an island that can be planted again.
    d.brush(at, 60., Some(5.), None).unwrap();
    d.brush(at, 60., None, Some(5)).unwrap();
    assert!(d.scene.is_burnable(c));
}

#[test]
fn road_vertices_can_be_inserted_moved_removed_and_objects_moved() {
    use scenario::editor::{blank, nearest_on_segment, Selection};
    let mut d = Document::new(blank("verts", 16, 20.).unwrap()).unwrap();
    let Selection::Road(i) = d.add("road", [20., 20.], Some([200., 20.])).unwrap() else {
        panic!("road expected")
    };
    let v = d.insert_road_vertex(i, 0, [100., 20.]).unwrap();
    assert_eq!((v, d.scene.roads[i].points.len()), (1, 3));
    d.move_road_vertex(i, v, [100., 120.]).unwrap();
    assert_eq!(d.scene.roads[i].points[1], [100., 120.]);
    assert!(d.move_road_vertex(i, v, [1e6, 0.]).is_err());
    assert!(d.insert_road_vertex(i, 5, [50., 50.]).is_err());
    d.remove_road_vertex(i, v).unwrap();
    assert!(d.remove_road_vertex(i, 0).is_err(), "two vertices stay");
    let b = d.add_building([60., 60.], Default::default()).unwrap();
    d.move_object(b, [120., 80.]).unwrap();
    let Selection::Building(bi) = b else { panic!() };
    assert_eq!(d.scene.buildings[bi].pos, [120., 80.]);
    assert!(d.move_object(Selection::Road(i), [10., 10.]).is_err());
    let (dist, q) = nearest_on_segment([50., 30.], [0., 0.], [100., 0.]);
    assert_eq!((dist, q), (30., [50., 0.]));
}

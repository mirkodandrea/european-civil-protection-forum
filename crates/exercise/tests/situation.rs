//! Impact picture, turn preview and inject suggestions, headless.
use std::collections::BTreeMap;
use std::sync::Arc;

use exercise::*;
use serde_json::json;
use sim::impact::ElementState;
use sim::{Command, World};

fn act(id: &str, offset_s: i64, wind: f64) -> scenario::InjectAction {
    scenario::InjectAction {
        id: id.into(),
        title: format!("Action {id}"),
        command: json!({"type": "SetWeather", "wind_dir_deg": wind}),
        offset_s,
        note: String::new(),
    }
}

/// Synthetic valley, started, with a pre-action at 300 s and a post-action of
/// an inject planned at 600 s.
fn started(turn_minutes: u32) -> Exercise {
    let mut scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
    for (n, i) in scn.msel.iter_mut().enumerate() {
        i.at_s = Some(100_000 + n as i64 * 60);
    }
    scn.msel[0].at_s = Some(600);
    scn.msel[0].actions = vec![act("late_wind", 120, 200.0), act("pre_wind", -300, 100.0)];
    scn.check().unwrap();
    let mut ex = Exercise::new(World::new(Arc::new(scn), 1).unwrap());
    ex.start().unwrap();
    ex.turn_minutes = turn_minutes;
    ex
}

fn approve_warning(ex: &mut Exercise) {
    let lang = ex.language_of("mayor");
    ex.set_language("mayor", &lang).unwrap();
    let parent = ex.send_inject(None, "Task", "EXCON", &["mayor".into()], "Task", &BTreeMap::new()).unwrap()[0];
    let pid = ex
        .player_reply("mayor", "warn F4", parent, DecisionEvidence::judgment("Operational judgment based on received information"))
        .unwrap();
    ex.attach_parse(
        pid,
        ParsedInput {
            resource_limit_draft: None,
            communication_drafts: vec![],
            language: "en".into(),
            translation_en: "warn".into(),
            actions: vec![json!({"type":"IssueWarning","zone":"F4"})],
            clarification: None,
            summary: "warn".into(),
            inject_drafts: vec![],
        },
    )
    .unwrap();
    ex.decide(pid, Decision { approve: true, commands: None, note: None, persona: None }).unwrap();
}

fn signature(ex: &Exercise) -> String {
    let w = &ex.world;
    format!(
        "{:?}|{}|{}|{:?}",
        w.stats(),
        w.fire_raster_b64(),
        serde_json::to_string(&w.d.roads).unwrap(),
        w.d.buildings
            .iter()
            .map(|b| (b.damage, b.threatened, b.people_inside, b.evacuated, b.at_risk))
            .collect::<Vec<_>>()
    )
}

/// The key invariant: the preview ends exactly where a real resolution of the
/// same turn ends, whatever is approved or planned.
#[test]
fn preview_equals_the_real_resolution() {
    let mut ex = started(60);
    approve_warning(&mut ex);
    let before = signature(&ex);
    let key = ex.preview_key();
    let preview = ex.preview_job().unwrap().run().unwrap();
    assert_eq!(signature(&ex), before, "a preview never changes the exercise");
    assert_eq!(ex.preview_key(), key);
    assert_eq!(ex.world.time_s(), 0);
    assert!(ex.action_runs.values().all(|r| r.executed_s.is_none()));
    assert!(preview.actions.iter().any(|a| a.text.contains("Action pre_wind")), "{:?}", preview.actions);
    assert_eq!(preview.applied.len(), 1, "the approved proposal is applied by the advance");

    ex.resolve_turn(None).unwrap();
    assert_eq!(ex.world.time_s(), preview.to_s);
    let real = ex.world.impact();
    assert_eq!(serde_json::to_value(&real.totals).unwrap(), serde_json::to_value(&preview.end.totals).unwrap());
    assert_eq!(serde_json::to_value(&real.rows).unwrap(), serde_json::to_value(&preview.end.rows).unwrap());
    assert_eq!(ex.world.fire_raster_b64(), preview.fire.raster);
    assert_eq!(preview.end.totals.roads_cut, real.totals.roads_cut);
}

#[test]
fn preview_respects_disabled_actions_and_skipped_injects() {
    let mut ex = started(30);
    let id = ex.world.scn.msel[0].id.clone();
    ex.set_action_enabled(&id, "pre_wind", false).unwrap();
    let preview = ex.preview_job().unwrap().run().unwrap();
    assert!(preview.actions.iter().all(|a| !a.text.contains("pre_wind")), "{:?}", preview.actions);
    ex.set_action_enabled(&id, "pre_wind", true).unwrap();
    ex.skip_inject(&id).unwrap();
    let skipped = ex.preview_job().unwrap().run().unwrap();
    assert!(skipped.actions.is_empty(), "a skipped inject cancels its pre-actions");
    assert_ne!(preview.key, skipped.key);
}

#[test]
fn preview_key_changes_with_decisions_and_time() {
    let mut ex = started(30);
    let k0 = ex.preview_key();
    ex.direct_command(&Command::IssueWarning { zone: "F4".into() }).unwrap();
    let k1 = ex.preview_key();
    assert_ne!(k0, k1);
    ex.begin_resolution_until(300, true).unwrap();
    while !ex.step_resolution(STEP_S).unwrap() {}
    ex.finish_resolution().unwrap();
    assert_ne!(ex.preview_key(), k1);
}

#[test]
fn preview_is_refused_outside_the_preparation_phase() {
    let ex = Exercise::new(
        World::new(Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED)), 1).unwrap(),
    );
    assert!(ex.preview_job().is_err());
}

#[test]
fn impact_report_counts_and_deltas() {
    let mut ex = started(60);
    let r0 = ex.impact_report();
    assert_eq!(r0.totals.buildings as usize, ex.world.scn.buildings.len());
    assert_eq!(r0.delta.threatened, 0);
    let people: i64 = ex.world.scn.buildings.iter().map(|b| b.occupants as i64).sum();
    assert_eq!(r0.totals.people_present + r0.totals.people_evacuating, people);
    assert_eq!(r0.places.iter().map(|p| p.counts.buildings).sum::<i64>(), r0.totals.buildings);
    // After some fire, the delta against the turn start shows the change.
    ex.begin_resolution_until(1800, true).unwrap();
    while !ex.step_resolution(STEP_S).unwrap() {}
    ex.finish_resolution().unwrap();
    let r1 = ex.impact_report();
    assert!(r1.totals.roads_total > 0);
    assert_eq!(r1.delta.threatened, r1.totals.threatened - r0.totals.threatened);
    assert_eq!(r1.delta.damaged, r1.totals.damaged);
}

/// A world where the fire certainly reaches a populated school.
fn threatened_world() -> Exercise {
    let mut ex = started(120);
    let b = ex
        .world
        .scn
        .buildings
        .iter()
        .find(|b| b.kind == scenario::BuildingKind::School)
        .cloned()
        .unwrap();
    let p = [b.pos[0] - 150.0, b.pos[1]];
    ex.direct_command(&Command::Ignite { at: sim::Loc::Point { x: p[0], y: p[1] }, radius_m: Some(60.0) }).unwrap();
    ex
}

#[test]
fn suggestions_flag_a_school_reached_before_it_is_evacuated() {
    let mut ex = threatened_world();
    let preview = ex.preview_job().unwrap().run().unwrap();
    let list = ex.suggestions(Some(&preview));
    let school = list.iter().find(|s| s.rule == "building_reached").unwrap_or_else(|| panic!("{list:#?}")).clone();
    assert_eq!(school.severity, exercise::suggestions::Severity::High);
    assert!(school.evidence.it.contains("persone") && school.evidence.en.contains("people"));
    assert!(school.evidence.en.contains("evacuation not started"));
    assert!(school.text.en.contains("Transport"));
    assert!(!school.recipients.is_empty());
    // Dismissing it hides exactly that (rule, object).
    ex.mark_suggestion(&school.id, "dismissed").unwrap();
    let again = ex.suggestions(Some(&preview));
    assert!(again.iter().all(|s| s.id != school.id));
    assert_eq!(again.len(), list.len() - 1);
    // A rollback to before the decision forgets the dismissal.
    assert!(ex.suggestion_marks.contains_key(&school.id));
    ex.rollback(1).unwrap();
    assert!(ex.suggestion_marks.is_empty());
}

#[test]
fn planned_injects_are_listed_and_never_sent() {
    let mut ex = started(60);
    let n = ex.world.scn.msel.len();
    let id = ex
        .add_planned_inject("Evacuate the school", "112", &["mayor".into()], "Call from the school", 1200, Some("building_reached:b1"))
        .unwrap();
    assert_eq!(ex.world.scn.msel.len(), n + 1);
    assert!(ex.suggestion_marks.contains_key("building_reached:b1"));
    let row = ex.planned_injects().into_iter().find(|i| i["id_inject"] == id.as_str()).unwrap();
    assert_eq!(row["scenario_time_s"], 1200);
    assert_eq!(row["sent"], false);
    // Advancing past it does not send it.
    ex.begin_resolution_until(1800, true).unwrap();
    while !ex.step_resolution(STEP_S).unwrap() {}
    ex.finish_resolution().unwrap();
    assert!(ex.inject_sent_to.get(&id).is_none());
    assert!(ex.add_planned_inject("x", "", &["nobody".into()], "t", 2000, None).is_err());
    assert!(ex.add_planned_inject("x", "", &["mayor".into()], "t", 1, None).is_err(), "not in the past");
}

#[test]
fn recipients_follow_role_capabilities() {
    let ex = started(30);
    let scn = &ex.world.scn;
    let r = exercise::suggestions::recipients_for(scn, &["EvacuateZone"]);
    assert!(r.iter().all(|id| scn.role(id).unwrap().allowed_actions.contains(&"EvacuateZone".to_string())));
    assert_eq!(exercise::suggestions::recipients_for(scn, &["NoSuchAction"]), vec!["all".to_string()]);
}

#[test]
fn preview_timelines_record_first_times_in_order() {
    let ex = threatened_world();
    let preview = ex.preview_job().unwrap().run().unwrap();
    assert!(!preview.timelines.is_empty());
    for (id, tl) in &preview.timelines {
        if let (Some(t), Some(d)) = (tl.threatened_s, tl.damaged_s) {
            assert!(t <= d, "{id}: threatened {t} after damaged {d}");
        }
        if let (Some(d), Some(x)) = (tl.damaged_s, tl.destroyed_s) {
            assert!(d <= x, "{id}");
        }
        for t in [tl.threatened_s, tl.damaged_s, tl.destroyed_s, tl.cut_s].into_iter().flatten() {
            assert!(t > preview.from_s && t <= preview.to_s);
        }
    }
    let now = ex.world.impact();
    for row in &preview.end.rows {
        if row.state >= ElementState::Damaged && row.kind == sim::impact::ElementKind::Building {
            let was = now.row(&row.id).map(|r| r.state).unwrap_or(ElementState::Normal);
            if was < ElementState::Damaged {
                assert!(preview.timelines[&row.id].damaged_s.is_some(), "{}", row.id);
            }
        }
    }
}

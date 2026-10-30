//! EXCON actions taken while orders are open wait for play; scheduled ones
//! run at their instant, independently of injects. Headless.
use std::sync::Arc;

use exercise::queue::{ExconOutcome, ExconWhen};
use exercise::*;
use serde_json::json;
use sim::{Command, Loc, World};

fn started() -> Exercise {
    let mut scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
    // No inject actions in the way: only the queue matters here.
    for (n, i) in scn.msel.iter_mut().enumerate() {
        i.at_s = Some(100_000 + n as i64 * 60);
        i.actions.clear();
    }
    scn.check().unwrap();
    let mut ex = Exercise::new(World::new(Arc::new(scn), 1).unwrap());
    ex.start().unwrap();
    ex.turn_minutes = 20;
    ex
}

/// An ignition on fuel, next to the school (as in the Situation tests).
fn ignite() -> Command {
    let scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
    let b = scn.buildings.iter().find(|b| b.category() == "school").unwrap();
    Command::Ignite { at: Loc::Point { x: b.pos[0] - 150.0, y: b.pos[1] }, radius_m: Some(60.0) }
}

fn burning(ex: &Exercise) -> bool {
    ex.world.stats().burning_cells > 0 || ex.world.stats().burned_ha > 0.0
}

fn queue(ex: &mut Exercise, cmd: &Command, when: ExconWhen) -> u64 {
    match ex.apply_or_queue_excon(cmd, when, json!({"origin": "test"})).unwrap() {
        ExconOutcome::Queued { id, .. } => id,
        o => panic!("expected a queued action, got {o:?}"),
    }
}

fn status(ex: &Exercise, id: u64) -> serde_json::Value {
    ex.excon_queue().into_iter().find(|r| r["id"] == id).unwrap()["status"].clone()
}

#[test]
fn an_action_during_orders_waits_for_play_and_applies_at_its_start() {
    let mut ex = started();
    let before = ex.world.view();
    let id = queue(&mut ex, &ignite(), ExconWhen::Play);
    assert_eq!(ex.world.view(), before, "nothing changes while players are deciding");
    assert!(!burning(&ex));
    let row = ex.excon_queue()[0].clone();
    assert_eq!(row["status"], "approved");
    assert_eq!(row["on_play"], true);
    assert_eq!(row["run_at_s"], 0);
    // The turn track shows it, never as something to handle.
    let agenda = ex.turn_agenda();
    let item = agenda["items"].as_array().unwrap().iter().find(|i| i["kind"] == "excon").unwrap().clone();
    assert_eq!((item["id"].clone(), item["due"].clone()), (json!(id), json!(false)));
    ex.begin_resolution(Some(1)).unwrap();
    // Applied at the start of the advance, before any simulated time passes.
    assert_eq!(ex.world.time_s(), 0);
    assert!(burning(&ex));
    assert_eq!(status(&ex, id), "done");
    while !ex.step_resolution(1).unwrap() {}
    ex.finish_resolution().unwrap();
    assert_eq!(status(&ex, id), "done");
}

#[test]
fn a_queued_action_can_be_edited_or_cancelled_before_play() {
    let mut ex = started();
    let a = queue(&mut ex, &ignite(), ExconWhen::Play);
    let b = queue(&mut ex, &Command::IssueWarning { zone: "F4".into() }, ExconWhen::Play);
    ex.edit_excon_action(b, Some(Command::IssueWarning { zone: "G4".into() }), Some(ExconWhen::At(300))).unwrap();
    let row = ex.excon_queue().into_iter().find(|r| r["id"] == b).unwrap();
    assert_eq!(row["run_at_s"], 300);
    assert_eq!(row["command"]["zone"], "G4");
    ex.cancel_excon_action(a, "changed my mind").unwrap();
    assert!(ex.cancel_excon_action(a, "twice").is_err());
    ex.resolve_turn(None).unwrap();
    assert_eq!(status(&ex, a), "cancelled");
    assert!(!burning(&ex), "the cancelled ignition never ran");
    assert_eq!(status(&ex, b), "done");
    assert!(ex.world.d.warned.contains_key("G4"));
}

#[test]
fn setup_bookkeeping_and_apply_now_stay_immediate() {
    let scn = Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED));
    let mut ex = Exercise::new(World::new(scn, 1).unwrap());
    // Before the start: setup applies at once.
    let o = ex.apply_or_queue_excon(&ignite(), ExconWhen::Play, json!({})).unwrap();
    assert!(matches!(o, ExconOutcome::Applied { .. }));
    ex.start().unwrap();
    // Time control and resource bookkeeping are not situation changes.
    let o = ex
        .apply_or_queue_excon(&Command::AdjustResource { role: "mayor".into(), resource: "buses".into(), delta: 1 }, ExconWhen::Play, json!({}))
        .unwrap();
    assert!(matches!(o, ExconOutcome::Applied { .. }));
    assert!(ex.apply_or_queue_excon(&Command::AdvanceTime { minutes: 1 }, ExconWhen::At(600), json!({})).is_err());
    // The explicit escape hatch.
    let o = ex.apply_or_queue_excon(&Command::IssueWarning { zone: "F4".into() }, ExconWhen::Now, json!({})).unwrap();
    assert!(matches!(o, ExconOutcome::Applied { .. }));
    assert!(ex.world.d.warned.contains_key("F4"));
    assert!(ex.excon_queue().is_empty());
    // A command the world refuses is refused when queued, not at play.
    assert!(ex
        .apply_or_queue_excon(&Command::WaterDrop { at: Loc::Named("Atlantis".into()) }, ExconWhen::Play, json!({}))
        .is_err());
    // The past cannot be scheduled.
    ex.resolve_turn(Some(5)).unwrap();
    assert!(ex.apply_or_queue_excon(&ignite(), ExconWhen::At(60), json!({})).is_err());
}

/// The ignition at a chosen instant, with no inject: it runs at that exact
/// second whatever the step size.
#[test]
fn a_scheduled_action_runs_at_its_instant_whatever_the_step() {
    let run = |step: i64| {
        let mut ex = started();
        let id = queue(&mut ex, &ignite(), ExconWhen::At(7 * 60 + 13));
        ex.begin_resolution(Some(5)).unwrap();
        while !ex.step_resolution(step).unwrap() {}
        ex.finish_resolution().unwrap();
        assert_eq!(status(&ex, id), "approved", "not yet: it is planned in the next advance");
        assert!(!burning(&ex));
        ex.begin_resolution(Some(10)).unwrap();
        while !ex.step_resolution(step).unwrap() {}
        ex.finish_resolution().unwrap();
        let row = ex.excon_queue().into_iter().find(|r| r["id"] == id).unwrap();
        assert_eq!(row["executed_s"], 7 * 60 + 13);
        (ex.world.fire_raster_b64(), burning(&ex))
    };
    let fine = run(1);
    assert!(fine.1);
    assert_eq!(run(60), fine, "step-size invariant");
    assert_eq!(run(7), fine, "step-size invariant");
}

#[test]
fn the_within_turn_clock_runs_queued_actions_at_their_instant() {
    let mut ex = started();
    let id = queue(&mut ex, &ignite(), ExconWhen::At(30));
    ex.configure_clock(true, 60.0, "live").unwrap();
    ex.advance_clock(20).unwrap();
    assert_eq!(status(&ex, id), "approved");
    ex.advance_clock(20).unwrap();
    let row = ex.excon_queue().into_iter().find(|r| r["id"] == id).unwrap();
    assert_eq!(row["executed_s"], 30);
}

#[test]
fn rollback_restores_the_queue() {
    let mut ex = started();
    // Turn 1: one for play, one scheduled into turn 2, one cancelled.
    let now = queue(&mut ex, &Command::IssueWarning { zone: "F4".into() }, ExconWhen::Play);
    let later = queue(&mut ex, &ignite(), ExconWhen::At(25 * 60));
    ex.resolve_turn(None).unwrap();
    assert_eq!(status(&ex, now), "done");
    assert_eq!(status(&ex, later), "approved");
    // Turn 2: queue another, then cancel the scheduled ignition.
    let t2 = queue(&mut ex, &Command::IssueWarning { zone: "G4".into() }, ExconWhen::Play);
    ex.cancel_excon_action(later, "not now").unwrap();
    assert!(ex.edit_excon_action(later, None, Some(ExconWhen::At(26 * 60))).is_err());
    ex.resolve_turn(None).unwrap();
    assert!(!burning(&ex));
    // Back to the start of turn 2: the cancellation is undone, the turn-2 action is gone.
    ex.rollback(2).unwrap();
    assert_eq!(status(&ex, later), "approved");
    assert!(ex.excon_queue().iter().all(|r| r["id"] != t2));
    assert_eq!(status(&ex, now), "done", "it ran before the restored instant");
    ex.resolve_turn(None).unwrap();
    assert!(burning(&ex), "the ignition runs again");
    assert_eq!(status(&ex, later), "done");
    // Back to turn 2 again: an action that ran after the restored instant waits again.
    ex.rollback(2).unwrap();
    assert_eq!(status(&ex, later), "approved");
    // Back to turn 1: everything queued in turn 1 is forgotten.
    ex.rollback(1).unwrap();
    assert!(ex.excon_queue().is_empty());
}

#[test]
fn the_preview_includes_queued_actions_like_the_real_resolution() {
    let mut ex = started();
    queue(&mut ex, &ignite(), ExconWhen::Play);
    queue(&mut ex, &Command::SetWeather { wind_dir_deg: Some(90.0), wind_speed_kmh: Some(40.0), moisture_pct: None }, ExconWhen::At(5 * 60));
    let key = ex.preview_key();
    let preview = ex.preview_job().unwrap().run().unwrap();
    assert!(!burning(&ex), "a preview never changes the exercise");
    assert_eq!(ex.preview_key(), key);
    assert_eq!(preview.actions.len(), 2, "{:?}", preview.actions);
    assert!(preview.actions.iter().all(|a| a.text.starts_with("EXCON:") && a.ok));
    assert_eq!(preview.actions[1].at_s, 5 * 60);
    ex.resolve_turn(None).unwrap();
    assert_eq!(ex.world.fire_raster_b64(), preview.fire.raster);
    assert_eq!(
        serde_json::to_value(&ex.world.impact().totals).unwrap(),
        serde_json::to_value(&preview.end.totals).unwrap()
    );
}

#[test]
fn editing_the_command_changes_the_preview_key() {
    let mut ex = started();
    let id = queue(&mut ex, &ignite(), ExconWhen::Play);
    let k = ex.preview_key();
    ex.edit_excon_action(id, Some(Command::Ignite { at: Loc::Named("G4".into()), radius_m: Some(80.0) }), None).unwrap();
    assert_ne!(ex.preview_key(), k);
}

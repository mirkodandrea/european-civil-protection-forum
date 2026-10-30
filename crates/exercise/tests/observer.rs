//! Observer dashboard: per-turn and cumulated counts, overall and per role.
use exercise::response::PlayerResponse;
use exercise::*;
use std::{collections::BTreeMap, sync::Arc};

fn exercise() -> Exercise {
    Exercise::new(
        sim::World::new(
            Arc::new(scenario::synthetic::generate(
                scenario::synthetic::DEFAULT_SEED,
            )),
            1,
        )
        .unwrap(),
    )
}
fn inject(ex: &mut Exercise, msel: Option<&str>, to: &str) -> u64 {
    ex.send_inject(msel, "Smoke over the village", "112", &[to.into()], "Residents report smoke.", &BTreeMap::new())
        .unwrap()[0]
}
fn reply(ex: &mut Exercise, role: &str, parent: u64, basis: BasisKind) -> u64 {
    ex.player_response(
        role,
        PlayerResponse {
            narrative: Some("We close the road".into()),
            narrative_en: "We close the road".into(),
            reply_to: Some(parent),
            basis: Some(basis),
            basis_note: (basis != BasisKind::OperationalJudgment).then(|| "Municipal plan §4".into()),
            rationale: Some("Smoke reaches the houses".into()),
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn counts_per_turn_cumulate_and_split_by_role_and_inject() {
    let mut ex = exercise();
    let before = ex.observer_dashboard();
    assert_eq!(before.started_wall_ms, None);
    assert_eq!(before.injects_sent, 0);
    assert_eq!(before.injects_total as usize, ex.world.scn.msel.len());

    ex.start().unwrap();
    let msel = ex.world.scn.msel.first().map(|t| t.id.clone());
    let first = inject(&mut ex, msel.as_deref(), "mayor");
    reply(&mut ex, "mayor", first, BasisKind::Plan);
    reply(&mut ex, "mayor", first, BasisKind::OperationalJudgment);
    let (cid, _) = ex
        .start_linked_conversation("mayor", first, &["regional_cp".into()], "Your view?", "Your view?", &BTreeMap::new(), false)
        .unwrap();
    ex.post_conversation_message("regional_cp", cid, "Close it", None, "Close it", &BTreeMap::new(), false)
        .unwrap();

    let open = ex.observer_dashboard();
    assert!(open.started_wall_ms.is_some());
    assert_eq!(open.turns.len(), 1);
    assert!(!open.turns[0].closed, "the open turn is provisional");
    let t1 = &open.turns[0].turn_only;
    assert_eq!(t1.overall.responses, 2);
    assert_eq!(t1.overall.interactions, 2);
    assert_eq!(t1.overall.conversations, 1);
    assert_eq!(t1.overall.basis.plan, 1);
    assert_eq!(t1.overall.basis.judgment, 1);
    assert_eq!(t1.roles["mayor"].responses, 2);
    assert_eq!(t1.roles["mayor"].received, 1);
    assert_eq!(t1.roles["regional_cp"].interactions, 1);
    assert_eq!(t1.roles["regional_cp"].responses, 0);
    let row = open.injects.iter().find(|i| i.sent_turn == Some(1)).unwrap();
    assert_eq!(row.msel, msel.is_some());
    assert_eq!(row.overall.responses, 2);
    assert_eq!(row.overall.interactions, 2);
    assert_eq!(row.roles["regional_cp"].interactions, 1);
    assert!(row.first_response_ms.is_some());
    if msel.is_some() {
        assert_eq!(open.injects_sent, 1);
        assert_eq!(t1.injects_sent, 1);
    } else {
        assert_eq!(t1.custom_injects, 1);
    }

    ex.resolve_turn(Some(1)).unwrap();
    let second = inject(&mut ex, None, "regional_cp");
    reply(&mut ex, "regional_cp", second, BasisKind::Procedure);

    let d = ex.observer_dashboard();
    assert_eq!(d.turn, 2);
    assert_eq!(d.turns.len(), 2);
    assert!(d.turns[0].closed && !d.turns[1].closed);
    assert!(d.turns[0].closed_wall_ms.is_some());
    assert_eq!(d.turns[1].start_s, d.turns[0].end_s.unwrap());
    // Turn 1 is final: unchanged by what happens in turn 2.
    assert_eq!(d.turns[0].turn_only.overall, open.turns[0].turn_only.overall);
    let t2 = &d.turns[1];
    assert_eq!(t2.turn_only.overall.responses, 1);
    assert_eq!(t2.turn_only.overall.basis.procedure, 1);
    assert_eq!(t2.cumulative.overall.responses, 3);
    assert_eq!(t2.cumulative.overall.interactions, 2);
    assert_eq!(t2.cumulative.roles["regional_cp"].responses, 1);
    assert_eq!(t2.cumulative.roles["mayor"].responses, 2);
    assert!(d.scenario.now_s >= d.scenario.turn_start_s);
    assert!(d.scenario.planned_turns >= 2);
}

#[test]
fn the_end_closes_the_last_turn_and_stops_the_timer() {
    let mut ex = exercise();
    ex.start().unwrap();
    ex.resolve_turn(Some(1)).unwrap();
    ex.end().unwrap();
    let d = ex.observer_dashboard();
    assert!(d.ended);
    assert!(d.ended_wall_ms.is_some());
    assert!(d.turns.iter().all(|t| t.closed), "the end freezes every turn");
}

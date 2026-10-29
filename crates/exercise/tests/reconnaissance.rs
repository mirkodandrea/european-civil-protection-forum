//! Ground reconnaissance (`Reconnoitre`) and role authority over player actions:
//! a role proposes only the action kinds its scenario role allows; a crew sent
//! to a zone reports once on arrival and updates only its owner's map.
use std::sync::Arc;

use exercise::*;
use sim::{Actor, Command, TaskStatus, Verdict, World};

fn exercise() -> Exercise {
    let mut scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
    for (n, i) in scn.msel.iter_mut().enumerate() {
        i.at_s = Some(100_000 + n as i64 * 60);
    }
    scn.check().unwrap();
    let mut ex = Exercise::new(World::new(Arc::new(scn), 1).unwrap());
    ex.start().unwrap();
    ex
}

fn recon(zone: &str) -> Command {
    Command::Reconnoitre { zone: zone.into(), role: None }
}

fn run_to(ex: &mut Exercise, to_s: i64) {
    ex.begin_resolution_until(to_s, true).unwrap();
    while !ex.step_resolution(30).unwrap() {}
    ex.finish_resolution().unwrap();
}

fn recon_events(ex: &Exercise, role: &str) -> Vec<KnowledgeEvent> {
    ex.knowledge_events
        .iter()
        .filter(|e| {
            e.role == role
                && matches!(e.source_name.as_deref(), Some("Reconnaissance crew" | "Squadra di ricognizione"))
        })
        .cloned()
        .collect()
}

#[test]
fn role_authority_follows_the_scenario_role() {
    let ex = exercise();
    let fc = ex.world.scn.role("fire_commander").unwrap();
    assert!(fc.allowed_actions.iter().any(|a| a == "Reconnoitre"), "the fire commander may reconnoitre");
    // Every kind outside a role's list is refused, every kind inside passes the authority check.
    for role in &ex.world.scn.roles {
        let who = Actor::Role(role.id.clone());
        for kind in sim::ALL_KINDS {
            let cmd = match *kind {
                "Reconnoitre" => recon("F3"),
                "IssueWarning" => Command::IssueWarning { zone: "F3".into() },
                "CloseRoad" => Command::CloseRoad { road: ex.world.scn.roads[0].id.clone() },
                _ => continue,
            };
            let verdict = ex.world.validate(&cmd, &who);
            let allowed = role.allowed_actions.iter().any(|a| a == kind);
            let refused = matches!(&verdict, Verdict::Rejected { reason } if reason.contains("not authorised"));
            assert_eq!(!refused, allowed, "{} {kind}: {verdict:?}", role.id);
        }
    }
    // The player page lists exactly the role's actions.
    let state = ex.player_static("mayor").unwrap();
    let mayor = ex.world.scn.role("mayor").unwrap();
    assert_eq!(state["roles"][0]["allowed_actions"], serde_json::json!(mayor.allowed_actions));
}

#[test]
fn a_crew_reports_once_on_arrival_and_stays_deployed() {
    let mut ex = exercise();
    let who = Actor::Role("fire_commander".into());
    let before = ex.world.d.pools["fire_commander"]["ground_crews"].available();
    let notes = ex.world.apply(&recon("F3"), &who).unwrap();
    assert!(notes.iter().any(|n| n.contains("Dispatched")), "{notes:?}");
    assert_eq!(ex.world.d.pools["fire_commander"]["ground_crews"].available(), before - 1);
    let task = ex.world.d.tasks.iter().find(|t| matches!(t.command, Command::Reconnoitre { .. })).unwrap().clone();
    assert_eq!(task.resource, "ground_crews");
    assert!(task.arrival_s > ex.world.time_s(), "the crew needs time to get there");
    // Nothing is learned in transit (no report before the arrival time).
    assert!(recon_events(&ex, "fire_commander").is_empty());

    run_to(&mut ex, task.arrival_s + 600);
    let events = recon_events(&ex, "fire_commander");
    assert_eq!(events.len(), 1, "one report, not a live feed");
    assert!(events[0].t_s >= task.arrival_s);
    assert!(ex.known_zones("fire_commander").contains(&"F3".to_string()));
    assert!(!ex.known_zones("mayor").contains(&"F3".to_string()), "only the owner learns");
    assert!(ex.messages.iter().any(|m| m.to == "fire_commander" && m.title.as_deref() == Some("Reconnaissance report")));
    let done = ex.world.d.tasks.iter().find(|t| t.id == task.id).unwrap();
    assert_eq!(done.status, TaskStatus::Completed);
    assert_eq!(
        ex.world.d.pools["fire_commander"]["ground_crews"].available(),
        before - 1,
        "the crew stays deployed until EXCON confirms its return"
    );
}

#[test]
fn excon_reconnaissance_names_the_informed_role_and_reports_at_once() {
    let mut ex = exercise();
    assert!(ex.direct_command(&recon("F3")).is_err(), "EXCON must say who is informed");
    ex.direct_command(&Command::Reconnoitre { zone: "F3".into(), role: Some("mayor".into()) }).unwrap();
    assert_eq!(recon_events(&ex, "mayor").len(), 1);
    assert!(ex.known_zones("mayor").contains(&"F3".to_string()));
    assert!(ex.world.validate(&recon("Z99"), &Actor::Role("fire_commander".into())) != Verdict::Ok { notes: vec![] });
}

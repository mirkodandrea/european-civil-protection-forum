//! Paired, reproducible operational experiments through the real World authority.
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sim::{Actor, Command, TaskStatus, World};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Suite {
    scenarios: Vec<Case>,
    seeds: Vec<u64>,
    duration_s: i64,
    #[serde(default = "sample_default")]
    sample_s: i64,
    #[serde(default = "realizations_default")]
    realizations: usize,
    #[serde(default = "cutoff_default")]
    cutoff: f32,
    variants: Vec<Variant>,
    strategies: Vec<Strategy>,
    /// Identical EXCON environment changes in the reference and every strategy.
    #[serde(default)]
    injects: Vec<Order>,
}
fn sample_default() -> i64 {
    60
}
fn realizations_default() -> usize {
    32
}
fn cutoff_default() -> f32 {
    0.5
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    path: String,
    #[serde(default)]
    injects: Vec<Order>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Variant {
    name: String,
    #[serde(default)]
    operational: bool,
    /// JSON pointers into the effective scenario, checked before every run.
    #[serde(default)]
    parameters: BTreeMap<String, Value>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Strategy {
    name: String,
    orders: Vec<Order>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Order {
    at_s: i64,
    actor: String,
    command: Command,
}
#[derive(Serialize)]
struct Metrics {
    stats: sim::Stats,
    people_inside: u32,
    people_in_transit: u32,
    people_arrived: u32,
    vehicles_waiting: usize,
    exposed_in_transit: u32,
    water_used_l: f32,
    work_m: f32,
    resource_units_used: u32,
    task_seconds: BTreeMap<String, u64>,
    inactivity_seconds: BTreeMap<String, u64>,
    withdrawals: usize,
    first_arrival_s: Option<i64>,
    first_useful_work_s: Option<i64>,
    overlapping_work_task_seconds: u64,
    accepted_orders: usize,
    rejected_orders: usize,
}
struct Run {
    metrics: Metrics,
    series: Vec<Value>,
    orders: Vec<Value>,
    events: Vec<Value>,
}
fn state_name(status: TaskStatus) -> String {
    serde_json::to_value(status)
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}
fn sample(world: &World, elapsed_s: i64) -> Value {
    let transit = world
        .d
        .traffic
        .vehicles
        .iter()
        .filter(|v| !v.completed && v.departed)
        .map(|v| v.people)
        .sum::<u32>();
    json!({"elapsed_s":elapsed_s,"stats":world.stats(),
        "people_inside":world.d.buildings.iter().map(|b| b.people_inside).sum::<u32>(),
        "people_in_transit":transit,
        "water_used_l":world.d.tasks.iter().map(|t| t.used_water_l).sum::<f32>(),
        "tasks":world.d.tasks.iter().map(|t| json!({"id":t.id,"status":t.status,"detail":t.detail,"work_m":t.work_m,"water_l":t.water_l})).collect::<Vec<_>>()})
}
fn run(scn: Arc<scenario::Scenario>, seed: u64, suite: &Suite, strategy: &Strategy) -> Result<Run> {
    let mut world = World::with_options(
        scn,
        seed,
        sim::FireOptions {
            realizations: suite.realizations,
            cutoff: suite.cutoff,
        },
    )?;
    let mut schedule: Vec<_> = suite.injects.iter().chain(&strategy.orders).collect();
    // Stable sort: environment injects precede participant orders at the same instant.
    schedule.sort_by_key(|o| o.at_s);
    let mut next = 0;
    let mut orders = Vec::new();
    let mut series = Vec::new();
    let mut task_seconds = BTreeMap::new();
    let mut inactivity_seconds = BTreeMap::new();
    let mut withdrawals = 0;
    let mut previous = BTreeMap::new();
    let mut progress = BTreeMap::new();
    let mut overlapping = 0;
    let mut first_arrival_s = None;
    let mut first_useful_work_s = None;
    for elapsed in 0..=suite.duration_s {
        while next < schedule.len() && schedule[next].at_s == elapsed {
            let order = schedule[next];
            let actor = if order.actor == "master" {
                Actor::Master
            } else {
                Actor::Role(order.actor.clone())
            };
            let result = world.apply(&order.command, &actor);
            orders.push(json!({"order":order,"accepted":result.is_ok(),"result":result.map_err(|e| e.to_string())}));
            next += 1;
        }
        if elapsed % suite.sample_s == 0 || elapsed == suite.duration_s {
            series.push(sample(&world, elapsed));
        }
        if elapsed == suite.duration_s {
            break;
        }
        for task in &world.d.tasks {
            let name = state_name(task.status);
            if !matches!(
                task.status,
                TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
            ) {
                *task_seconds.entry(name.clone()).or_insert(0) += 1;
            }
            if matches!(task.status, TaskStatus::Working)
                && progress.get(&task.id) == Some(&(task.work_m, task.used_water_l))
            {
                *inactivity_seconds.entry(task.detail.clone()).or_insert(0) += 1;
            }
            if task.status == TaskStatus::Withdrawing && previous.get(&task.id) != Some(&name) {
                withdrawals += 1;
            }
            previous.insert(task.id, name);
            progress.insert(task.id, (task.work_m, task.used_water_l));
            if task.arrived_s.is_some() && first_arrival_s.is_none() {
                first_arrival_s = Some(elapsed);
            }
            if (task.work_m > 0.0 || task.used_water_l > 0.0) && first_useful_work_s.is_none() {
                first_useful_work_s = Some(elapsed);
            }
        }
        if first_useful_work_s.is_none() && (!world.d.lines.is_empty() || !world.d.drops.is_empty())
        {
            first_useful_work_s = Some(elapsed);
        }
        let working: Vec<_> = world
            .d
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Working)
            .collect();
        for (i, task) in working.iter().enumerate() {
            // Geometric diagnostic only: proximity does not imply duplicate physical effect.
            if working.iter().enumerate().any(|(j, other)| {
                i != j
                    && sim::network::distance(
                        task.position(world.time_s()),
                        other.position(world.time_s()),
                    ) < 60.0
            }) {
                overlapping += 1;
            }
        }
        world.advance(1)?;
    }
    let m = sample(&world, suite.duration_s);
    let metrics = Metrics {
        stats: world.stats(),
        people_inside: world.d.buildings.iter().map(|b| b.people_inside).sum(),
        people_in_transit: m["people_in_transit"].as_u64().unwrap() as u32,
        people_arrived: world.d.buildings.iter().map(|b| b.evacuated).sum(),
        vehicles_waiting: world
            .d
            .traffic
            .vehicles
            .iter()
            .filter(|v| v.waiting && !v.completed)
            .count(),
        exposed_in_transit: world
            .d
            .traffic
            .vehicles
            .iter()
            .filter(|v| v.at_risk)
            .map(|v| v.people)
            .sum(),
        water_used_l: world.d.tasks.iter().map(|t| t.used_water_l).sum(),
        work_m: world.d.tasks.iter().map(|t| t.work_m).sum(),
        resource_units_used: world
            .d
            .pools
            .values()
            .flat_map(|p| p.values())
            .map(|p| p.used)
            .sum(),
        task_seconds,
        inactivity_seconds,
        withdrawals,
        first_arrival_s,
        first_useful_work_s,
        overlapping_work_task_seconds: overlapping,
        accepted_orders: orders.iter().filter(|o| o["accepted"] == true).count(),
        rejected_orders: orders.iter().filter(|o| o["accepted"] == false).count(),
    };
    Ok(Run {
        metrics,
        series,
        orders,
        events: world
            .d
            .events
            .iter()
            .map(|e| serde_json::to_value(e).unwrap())
            .collect(),
    })
}
fn validate(suite: &Suite) -> Result<()> {
    ensure!(
        suite.duration_s > 0 && suite.duration_s <= 1_209_600,
        "duration_s must be in 1..=1209600"
    );
    ensure!(
        suite.sample_s > 0
            && suite.realizations > 0
            && suite.realizations <= 1024
            && suite.cutoff > 0.0
            && suite.cutoff <= 1.0,
        "invalid sampling or fire options"
    );
    ensure!(
        !suite.seeds.is_empty() && !suite.scenarios.is_empty() && !suite.variants.is_empty(),
        "empty suite dimensions"
    );
    ensure!(
        suite.seeds.iter().collect::<BTreeSet<_>>().len() == suite.seeds.len(),
        "duplicate seeds"
    );
    for names in [
        suite.scenarios.iter().map(|x| &x.name).collect::<Vec<_>>(),
        suite.variants.iter().map(|x| &x.name).collect(),
        suite.strategies.iter().map(|x| &x.name).collect(),
    ] {
        ensure!(
            names.iter().all(|n| !n.is_empty())
                && names.iter().collect::<BTreeSet<_>>().len() == names.len(),
            "empty or duplicate names"
        );
    }
    ensure!(
        !suite.strategies.iter().any(|s| s.name == "no_action"),
        "no_action is a reserved reference name"
    );
    for order in suite
        .injects
        .iter()
        .chain(suite.scenarios.iter().flat_map(|s| &s.injects))
        .chain(suite.strategies.iter().flat_map(|s| &s.orders))
    {
        ensure!(
            order.at_s >= 0 && order.at_s <= suite.duration_s,
            "order outside experiment duration"
        );
        ensure!(
            !matches!(order.command, Command::AdvanceTime { .. }),
            "AdvanceTime is forbidden: schedule commands with at_s"
        );
        ensure!(!order.actor.is_empty(), "empty actor");
    }
    ensure!(
        suite
            .injects
            .iter()
            .chain(suite.scenarios.iter().flat_map(|c| &c.injects))
            .all(|o| o.actor == "master"),
        "shared environment injects must be EXCON/master commands"
    );
    Ok(())
}
fn effective(base: &scenario::Scenario, variant: &Variant) -> Result<scenario::Scenario> {
    let mut scn = base.clone();
    if variant.operational {
        scn.enable_operational_model();
    }
    let mut value = serde_json::to_value(&scn)?;
    for (pointer, replacement) in &variant.parameters {
        let slot = value
            .pointer_mut(pointer)
            .with_context(|| format!("unknown scenario parameter {pointer}"))?;
        *slot = replacement.clone();
    }
    let scn: scenario::Scenario = serde_json::from_value(value)?;
    scn.check()?;
    Ok(scn)
}
fn save(path: impl AsRef<Path>, value: &impl Serialize) -> Result<()> {
    std::fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}
fn csv(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        bail!("usage: operational_lab SUITE_JSON NEW_OUTPUT_DIRECTORY");
    }
    let suite_path = Path::new(&args[0]);
    let suite: Suite = serde_json::from_slice(&std::fs::read(suite_path)?)?;
    validate(&suite)?;
    let out = Path::new(&args[1]);
    ensure!(
        !out.exists(),
        "output directory already exists; choose a new directory to preserve evidence"
    );
    std::fs::create_dir_all(out)?;
    let baseline = Strategy {
        name: "no_action".into(),
        orders: vec![],
    };
    let mut replay = suite.clone();
    replay.scenarios.clear();
    replay.variants = vec![Variant {
        name: "effective".into(),
        operational: false,
        parameters: BTreeMap::new(),
    }];
    let mut results = Vec::new();
    let mut table = String::from("scenario,variant,seed,strategy,burned_ha,burned_ha_avoided,damaged,destroyed,evacuated_people,people_at_risk,people_in_transit,water_used_l,work_m,withdrawals,rejected_orders\n");
    let mut rejected = 0;
    for (ci, case) in suite.scenarios.iter().enumerate() {
        let mut case_suite = suite.clone();
        case_suite.injects.extend(case.injects.clone());
        let base = scenario::Scenario::load(
            suite_path
                .parent()
                .unwrap_or(Path::new("."))
                .join(&case.path),
        )?;
        for (vi, variant) in suite.variants.iter().enumerate() {
            let scn = Arc::new(effective(&base, variant)?);
            let scenario_file = format!("scenario-{ci:03}-{vi:03}.json");
            save(out.join(&scenario_file), scn.as_ref())?;
            replay.scenarios.push(Case {
                name: format!("{} / {}", case.name, variant.name),
                path: scenario_file,
                injects: case.injects.clone(),
            });
            for &seed in &suite.seeds {
                let reference = run(scn.clone(), seed, &case_suite, &baseline)?;
                let reference_ha = reference.metrics.stats.burned_ha;
                for strategy in std::iter::once(&baseline).chain(&suite.strategies) {
                    let current = if strategy.name == "no_action" {
                        None
                    } else {
                        Some(run(scn.clone(), seed, &case_suite, strategy)?)
                    };
                    let run = current.as_ref().unwrap_or(&reference);
                    let m = &run.metrics;
                    rejected += m.rejected_orders;
                    let index = results.len();
                    let trace = format!("run-{index:05}.json");
                    save(
                        out.join(&trace),
                        &json!({"scenario":case.name,"variant":variant,"seed":seed,"strategy":strategy,"metrics":m,"series":run.series,"orders":run.orders,"events":run.events}),
                    )?;
                    table.push_str(&format!(
                        "{},{},{},{},{:.6},{:.6},{},{},{},{},{},{:.3},{:.3},{},{}\n",
                        csv(&case.name),
                        csv(&variant.name),
                        seed,
                        csv(&strategy.name),
                        m.stats.burned_ha,
                        reference_ha - m.stats.burned_ha,
                        m.stats.damaged,
                        m.stats.destroyed,
                        m.stats.evacuated_people,
                        m.stats.people_at_risk,
                        m.people_in_transit,
                        m.water_used_l,
                        m.work_m,
                        m.withdrawals,
                        m.rejected_orders
                    ));
                    results.push(json!({"scenario":case.name,"variant":variant.name,"seed":seed,"strategy":strategy.name,"metrics":m,"paired_reference":{"burned_ha":reference_ha,"burned_ha_avoided":reference_ha-m.stats.burned_ha,"destroyed_avoided":reference.metrics.stats.destroyed as i64-m.stats.destroyed as i64,"people_at_risk_avoided":reference.metrics.stats.people_at_risk as i64-m.stats.people_at_risk as i64},"trace":trace}));
                    println!(
                        "{} / {} / seed {} / {}: {:.2} ha, {} evacuated, {} rejected",
                        case.name,
                        variant.name,
                        seed,
                        strategy.name,
                        m.stats.burned_ha,
                        m.stats.evacuated_people,
                        m.rejected_orders
                    );
                }
            }
        }
    }
    // Identify the actual executable even when experiments use an uncommitted tree.
    let executable = std::fs::read(std::env::current_exe()?)?;
    let binary_fingerprint = executable.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty());
    save(out.join("suite.json"), &replay)?;
    save(
        out.join("manifest.json"),
        &json!({"schema":"ttx-operational-lab/v1","git_revision":revision,"working_tree_dirty":dirty,"simulator_binary":{"fingerprint_algorithm":"fnv1a64","fingerprint":format!("{binary_fingerprint:016x}"),"bytes":executable.len(),"package_version":env!("CARGO_PKG_VERSION"),"arch":std::env::consts::ARCH,"os":std::env::consts::OS},"original_suite":suite,"replay_suite":"suite.json","simulation_step_s":1,"results":results,"rejected_orders":rejected,"interpretation":"Paired model counterfactuals; not causal attribution or participant scores. Replay requires the same simulator revision and configuration."}),
    )?;
    std::fs::write(out.join("metrics.csv"), table)?;
    println!(
        "{} runs; {} rejected commands. Evidence: {}",
        results.len(),
        rejected,
        out.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn suite() -> Suite {
        Suite {
            scenarios: vec![Case {
                name: "valley".into(),
                path: "unused".into(),
                injects: vec![],
            }],
            seeds: vec![3],
            duration_s: 90,
            sample_s: 30,
            realizations: 1,
            cutoff: 0.5,
            variants: vec![Variant {
                name: "base".into(),
                operational: false,
                parameters: BTreeMap::new(),
            }],
            strategies: vec![],
            injects: vec![],
        }
    }
    #[test]
    fn schedules_reject_clock_mutation_and_unknown_parameters() {
        let mut s = suite();
        s.injects.push(Order {
            at_s: 0,
            actor: "master".into(),
            command: Command::AdvanceTime { minutes: 1 },
        });
        assert!(validate(&s).is_err());
        let scn = scenario::synthetic::generate(1);
        let mut v = s.variants.remove(0);
        v.parameters.insert("/operations/typo".into(), json!(true));
        assert!(effective(&scn, &v).is_err());
    }
    #[test]
    fn actual_world_orders_replay_and_preserve_paired_reference() {
        let scn = Arc::new(scenario::synthetic::generate(1));
        let s = suite();
        let baseline = Strategy {
            name: "test".into(),
            orders: vec![],
        };
        let first = run(scn.clone(), 3, &s, &baseline).unwrap();
        let second = run(scn.clone(), 3, &s, &baseline).unwrap();
        assert_eq!(
            serde_json::to_value(first.metrics).unwrap(),
            serde_json::to_value(second.metrics).unwrap()
        );
        let strategy = Strategy {
            name: "warning".into(),
            orders: vec![
                Order {
                    at_s: 30,
                    actor: "mayor".into(),
                    command: Command::IssueWarning { zone: "F4".into() },
                },
                Order {
                    at_s: 31,
                    actor: "mayor".into(),
                    command: Command::Ignite {
                        at: sim::Loc::Named("Pine Hill".into()),
                        radius_m: None,
                    },
                },
            ],
        };
        let r = run(scn, 3, &s, &strategy).unwrap();
        assert_eq!(r.metrics.accepted_orders, 1);
        assert_eq!(r.metrics.rejected_orders, 1);
        assert_eq!(r.orders[0]["order"]["at_s"], 30);
    }
    #[test]
    fn enhanced_telemetry_sampling_does_not_change_world_or_resource_effects() {
        let mut scn = scenario::synthetic::generate(1);
        scn.enable_operational_model();
        let scn = Arc::new(scn);
        let mut s = suite();
        s.duration_s = 600;
        let strategy = Strategy {
            name: "engine".into(),
            orders: vec![Order {
                at_s: 0,
                actor: "fire_commander".into(),
                command: Command::DeployEngine {
                    at: sim::Loc::Point {
                        x: 2940.0,
                        y: 2460.0,
                    },
                },
            }],
        };
        let sparse = run(scn.clone(), 3, &s, &strategy).unwrap();
        s.sample_s = 1;
        let dense = run(scn, 3, &s, &strategy).unwrap();
        assert_eq!(sparse.metrics.rejected_orders, 0);
        assert_eq!(
            serde_json::to_value(sparse.metrics).unwrap(),
            serde_json::to_value(dense.metrics).unwrap()
        );
        assert_eq!(dense.series.len(), 601);
    }
}

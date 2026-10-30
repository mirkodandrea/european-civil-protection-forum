//! Independent training phases, wall/simulation clock controls, and timed injects.
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::{Exercise, Phase, MASTER};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExercisePhase { EarlyWarning, EarlyAction, Escalation, FullResponse }

#[derive(Debug, Clone, Serialize)]
pub struct PhaseEntry { pub phase: ExercisePhase, pub turn: u32, pub t_s: i64, pub wall_ms: u128, pub reason: String }

#[derive(Debug, Clone, Serialize)]
pub struct ClockControl {
    pub running: bool,
    /// Simulated seconds per wall-clock second; distinct from resolution playback speed.
    pub ratio: f64,
    pub revision: u64,
    pub lag_s: i64,
}
impl Default for ClockControl { fn default() -> Self { Self {running:false,ratio:60.0,revision:0,lag_s:0} } }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleStatus { Pending, Dispatching, Delivered, Failed, Cancelled }

#[derive(Debug, Clone, Serialize)]
pub struct ScheduledInject {
    pub id: u64,
    pub msel_id: String,
    pub due_s: i64,
    pub due_clock: String,
    pub created_phase: ExercisePhase,
    pub status: ScheduleStatus,
    pub triggered_s: Option<i64>,
    pub triggered_wall_ms: Option<u128>,
    pub delivered_s: Option<i64>,
    pub delivered_wall_ms: Option<u128>,
    pub messages: Vec<u64>,
    pub error: Option<String>,
}

impl Exercise {
    pub fn configure_clock(&mut self, running: bool, ratio: f64, reason: &str) -> Result<()> {
        if !ratio.is_finite() || !(0.01..=86400.0).contains(&ratio) { bail!("clock ratio must be 0.01–86400 simulated seconds per real second"); }
        if running && self.phase != Phase::Orders { bail!("the within-turn clock runs only while a turn is open"); }
        if self.phase == Phase::Debrief { bail!("the exercise has ended"); }
        let before = self.clock_control.clone();
        self.clock_control.running=running;self.clock_control.ratio=ratio;self.clock_control.lag_s=0;self.clock_control.revision+=1;
        self.record("clock_configured", MASTER, json!({"before":before,"after":self.clock_control,"reason":reason}));
        Ok(())
    }

    pub(crate) fn stop_clock(&mut self, reason: &str) {
        if self.clock_control.running {
            self.clock_control.running=false;self.clock_control.lag_s=0;self.clock_control.revision+=1;
            self.record("clock_paused", MASTER, json!({"reason":reason}));
        } else { self.clock_control.revision+=1; }
    }

    pub fn set_exercise_phase(&mut self, phase: ExercisePhase, reason: &str) -> Result<()> {
        if self.phase != Phase::Orders { bail!("training phase changes require an open turn"); }
        if reason.trim().is_empty() || reason.chars().count() > 2000 { bail!("phase transition requires a reason (up to 2000 characters)"); }
        self.validate_limits_for_phase(&self.resource_limits,phase)?;
        let from=self.exercise_phase;
        self.exercise_phase=phase;
        let entry=PhaseEntry {phase,turn:self.turn,t_s:self.world.time_s(),wall_ms:crate::wall_ms(),reason:reason.into()};
        self.phase_history.push(entry.clone());
        self.record("exercise_phase_changed",MASTER,json!({"from":from,"entry":entry}));
        Ok(())
    }

    /// Advance physics while Orders remains open. Never executes unapproved orders
    /// or opens a new turn. Small steps stop exactly at each scheduled due instant.
    pub fn advance_clock(&mut self, seconds: i64) -> Result<Vec<sim::SimEvent>> {
        if self.phase != Phase::Orders || !self.clock_control.running { bail!("within-turn clock is paused"); }
        if !(1..=60).contains(&seconds) { bail!("clock step must be 1–60 seconds"); }
        let resource_event_count=self.world.d.resource_events.len();
        // Queued EXCON actions and inject actions run at their instants, as in a resolution.
        let mut events=Vec::new();
        let mut left=seconds;
        loop {
            self.run_approved_actions();
            if left<=0 { break; }
            let dt=self.next_action_in().map_or(left,|n|n.min(left));
            events.extend(self.world.advance(dt)?);
            left-=dt;
        }
        self.record_resource_events(resource_event_count,MASTER);
        self.absorb_monitoring();
        if !events.is_empty() { self.record("clock_physical_events",MASTER,json!({"events":events})); } else { self.touch(); }
        Ok(events)
    }

    pub fn schedule_inject(&mut self, msel_id: &str, due_s: i64) -> Result<u64> {
        if self.phase == Phase::Debrief { bail!("the exercise has ended"); }
        if !self.world.scn.msel.iter().any(|i| i.id==msel_id) { bail!("unknown MSEL inject {msel_id}"); }
        if due_s < self.world.time_s() || due_s > 7*86400 { bail!("scheduled time must be current/future time within seven simulated days"); }
        let id=self.id();
        let schedule=ScheduledInject {id,msel_id:msel_id.into(),due_s,due_clock:scenario::format_clock(self.world.scn.start_clock_s+due_s),created_phase:self.exercise_phase,status:ScheduleStatus::Pending,triggered_s:None,triggered_wall_ms:None,delivered_s:None,delivered_wall_ms:None,messages:Vec::new(),error:None};
        self.record("inject_scheduled",MASTER,json!(&schedule));self.scheduled_injects.push(schedule);
        Ok(id)
    }

    pub fn cancel_schedule(&mut self, id: u64, reason: &str) -> Result<()> {
        if reason.trim().is_empty() { bail!("cancellation requires a reason"); }
        let schedule=self.scheduled_injects.iter_mut().find(|i| i.id==id).ok_or_else(||anyhow::anyhow!("unknown scheduled inject"))?;
        if !matches!(schedule.status,ScheduleStatus::Pending|ScheduleStatus::Dispatching) { bail!("schedule already completed"); }
        schedule.status=ScheduleStatus::Cancelled;self.record("inject_schedule_cancelled",MASTER,json!({"schedule":id,"reason":reason}));Ok(())
    }

    /// Claim each due delivery once. Translation happens outside the exercise lock.
    pub fn claim_due_injects(&mut self) -> Vec<(u64, scenario::InjectTemplate)> {
        if !matches!(self.phase,Phase::Orders|Phase::Resolving) { return Vec::new(); }
        let now=self.world.time_s();
        let mut due=self.scheduled_injects.iter().filter(|i| i.status==ScheduleStatus::Pending && i.due_s<=now).map(|i|(i.due_s,i.id)).collect::<Vec<_>>();due.sort();
        let mut claimed=Vec::new();
        for (_,id) in due {
            let i=self.scheduled_injects.iter_mut().find(|i|i.id==id).unwrap();i.status=ScheduleStatus::Dispatching;i.triggered_s=Some(now);i.triggered_wall_ms=Some(crate::wall_ms());
            let template=self.world.scn.msel.iter().find(|t|t.id==i.msel_id).unwrap().clone();
            self.record("scheduled_inject_triggered",MASTER,json!({"schedule":id,"due_s":self.scheduled_injects.iter().find(|i|i.id==id).unwrap().due_s,"triggered_s":now}));claimed.push((id,template));
        }
        claimed
    }

    /// Record actual delivery, including any translation/simulation lag. Cancellation
    /// and exercise end are rechecked by the server before sending the message.
    pub fn complete_schedule(&mut self, id:u64, result:Result<Vec<u64>>) {
        let Some(i)=self.scheduled_injects.iter_mut().find(|i|i.id==id) else{return};
        if i.status!=ScheduleStatus::Dispatching{return;}
        match result {
            Ok(messages)=>{i.status=ScheduleStatus::Delivered;i.delivered_s=Some(self.world.time_s());i.delivered_wall_ms=Some(crate::wall_ms());i.messages=messages;}
            Err(e)=>{i.status=ScheduleStatus::Failed;i.error=Some(e.to_string());}
        }
        let data=json!(&i);self.record("scheduled_inject_completed",MASTER,data);
    }
}

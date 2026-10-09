//! Original exercise design and explicit, evidence-linked path approvals.
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::{Exercise, ExercisePhase, Phase, MASTER};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrainingDesign {
    pub objectives: Vec<String>,
    pub phases: std::collections::BTreeMap<ExercisePhase, String>,
    pub decision_points: Vec<DecisionPoint>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionPoint { pub id: String, pub phase: ExercisePhase, pub branches: Vec<Branch> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Branch { pub path: String, pub phase: ExercisePhase, pub conditions: Vec<Condition> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag="type",rename_all="snake_case",deny_unknown_fields)]
pub enum Condition { BurningAtMost { cells: usize }, BurningAtLeast { cells: usize }, BurnedAtMost { ha: f64 }, BurnedAtLeast { ha: f64 }, ExecutedAction { kind: String } }

impl Exercise {
    pub fn configure_design(&mut self, design: TrainingDesign) -> Result<()> {
        if self.phase != Phase::Briefing { bail!("original design is fixed after exercise start; use explicit path adaptations"); }
        if design.objectives.len()>30 || design.decision_points.len()>30 || design.objectives.iter().chain(design.phases.values()).any(|s|s.trim().is_empty()||s.len()>4000) { bail!("invalid or oversized original design"); }
        let mut ids=std::collections::BTreeSet::new();
        for p in &design.decision_points {
            if p.id.trim().is_empty() || p.id.len()>100 || !ids.insert(&p.id) || p.branches.is_empty() || p.branches.len()>10 { bail!("invalid decision point"); }
            let mut paths=std::collections::BTreeSet::new();
            for b in &p.branches {
                if b.path.trim().is_empty()||b.path.len()>100||!paths.insert(&b.path)||b.conditions.is_empty()||b.conditions.len()>10 { bail!("invalid path or empty transition conditions"); }
                for c in &b.conditions { match c {
                    Condition::BurnedAtMost {ha}|Condition::BurnedAtLeast {ha} if !ha.is_finite()||*ha<0.0 => bail!("invalid burned area threshold"),
                    Condition::ExecutedAction {kind} if !sim::ALL_KINDS.contains(&kind.as_str()) => bail!("unknown action condition"),
                    _=>{},
                } }
            }
        }
        self.record("training_design_configured",MASTER,json!({"before":self.training_design,"after":design}));
        self.training_design=design;
        Ok(())
    }
    fn condition_evidence(&self,c:&Condition)->Value {
        let st=self.world.stats();
        let (met,evidence)=match c {
            Condition::BurningAtMost {cells}=>(st.burning_cells<=*cells,json!({"burning_cells":st.burning_cells})),
            Condition::BurningAtLeast {cells}=>(st.burning_cells>=*cells,json!({"burning_cells":st.burning_cells})),
            Condition::BurnedAtMost {ha}=>(st.burned_ha<=*ha,json!({"burned_ha":st.burned_ha})),
            Condition::BurnedAtLeast {ha}=>(st.burned_ha>=*ha,json!({"burned_ha":st.burned_ha})),
            Condition::ExecutedAction {kind}=>{
                let ids=self.proposals.iter().filter(|p|p.status!=crate::ProposalStatus::RolledBack && p.decision.as_ref().is_some_and(|d|d.approve) && p.executions.iter().any(|e|e.ok && e.command.kind()==kind)).map(|p|p.id).collect::<Vec<_>>();
                (!ids.is_empty(),json!({"approved_executed_proposals":ids}))
            },
        };
        json!({"condition":c,"met":met,"observed":evidence})
    }
    pub fn design_view(&self)->Value {
        json!({"original":self.training_design,"current_path":self.current_path,"current_phase":self.exercise_phase,"history":self.path_history,
            "decision_points":self.training_design.decision_points.iter().map(|p|json!({"id":p.id,"phase":p.phase,"in_current_phase":p.phase==self.exercise_phase,"branches":p.branches.iter().map(|b|json!({"path":b.path,"phase":b.phase,"conditions":b.conditions.iter().map(|c|self.condition_evidence(c)).collect::<Vec<_>>()})).collect::<Vec<_>>()})).collect::<Vec<_>>()})
    }
    pub fn approve_path(&mut self,point:&str,path:&str,reason:&str,adaptation:bool)->Result<()> {
        if self.phase!=Phase::Orders || reason.trim().is_empty()||reason.len()>2000 { bail!("path approval requires an open turn and rationale"); }
        let p=self.training_design.decision_points.iter().find(|p|p.id==point).ok_or_else(||anyhow::anyhow!("unknown decision point"))?;
        let b=p.branches.iter().find(|b|b.path==path).ok_or_else(||anyhow::anyhow!("unknown path"))?;
        let observations=b.conditions.iter().map(|c|self.condition_evidence(c)).collect::<Vec<_>>();
        let divergence=p.phase!=self.exercise_phase || observations.iter().any(|e|e["met"]!=true);
        if divergence && !adaptation { bail!("unplanned divergence: conditions/phase do not match; explicit Master adaptation required"); }
        let to=b.phase;
        self.validate_limits_for_phase(&self.resource_limits,to)?;
        let evidence=json!({"point":point,"from_path":self.current_path,"path":path,"from_phase":self.exercise_phase,"to_phase":to,"observations":observations,"deviation":divergence,"adaptation":adaptation,"reason":reason,"approved_by":MASTER,"turn":self.turn,"t_s":self.world.time_s(),"wall_ms":crate::wall_ms()});
        self.set_exercise_phase(to,reason)?;
        self.current_path=Some(path.into());
        self.path_history.push(evidence.clone());
        self.record("training_path_approved",MASTER,evidence);
        Ok(())
    }
}

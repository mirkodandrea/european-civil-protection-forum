//! Deployable capacity caps above physical inventory, enforced on active use and reservations.
use std::collections::BTreeMap;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sim::{Actor, Command, Pool, Verdict};
use crate::{Exercise, ExercisePhase, ProposalStatus, Source, MASTER};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceLimits {
    #[serde(default)] pub total: BTreeMap<String,u32>,
    #[serde(default)] pub phases: BTreeMap<ExercisePhase,BTreeMap<String,u32>>,
}

impl Exercise {
    pub fn effective_capacity(&self, resource:&str, phase:ExercisePhase, limits:&ResourceLimits)->u64 {
        let inventory=self.world.d.pools.values().filter_map(|p|p.get(resource)).map(|p|p.total as u64).sum::<u64>();
        inventory.min(limits.total.get(resource).copied().map(u64::from).unwrap_or(u64::MAX)).min(limits.phases.get(&phase).and_then(|p|p.get(resource)).copied().map(u64::from).unwrap_or(u64::MAX))
    }

    pub fn resource_usage_with_commitments(&self, except:u64)->BTreeMap<String,u64> {
        let mut used=BTreeMap::new();
        for pools in self.world.d.pools.values(){for (resource,pool) in pools{*used.entry(resource.clone()).or_default()+=pool.used as u64;}}
        for p in self.proposals.iter().filter(|p|p.status==ProposalStatus::Approved && p.id!=except){
            for c in &p.commands{for (r,qty) in self.world.resource_requirements(c,&p.source.actor()){*used.entry(r).or_default()+=qty as u64;}}
        }
        used
    }

    pub fn validate_limits_for_phase(&self, limits:&ResourceLimits, phase:ExercisePhase)->Result<()> {
        for r in limits.total.keys().chain(limits.phases.values().flat_map(|p|p.keys())) {
            if !self.world.scn.resource_types.iter().any(|t|t.id==*r){bail!("unknown resource {r}");}
        }
        for (resource,used) in self.resource_usage_with_commitments(0){
            let cap=self.effective_capacity(&resource,phase,limits);
            if used>cap{bail!("{resource}: {used} deployed/committed exceeds effective capacity {cap} for {phase:?}");}
        }
        Ok(())
    }

    pub fn set_resource_limits(&mut self, limits:ResourceLimits, reason:&str, proposal:Option<u64>)->Result<()> {
        if reason.trim().is_empty() || reason.chars().count()>2000{bail!("limit change requires a reason (up to 2000 characters)");}
        self.validate_limits_for_phase(&limits,self.exercise_phase)?;
        self.record("resource_limits_changed",MASTER,json!({"before":self.resource_limits,"after":limits,"reason":reason,"approved_by":MASTER,"proposal":proposal}));
        self.resource_limits=limits;Ok(())
    }

    pub fn edit_resource_limit_draft(&mut self,id:u64,limits:ResourceLimits)->Result<()> {
        let p=self.proposal(id).ok_or_else(||anyhow::anyhow!("unknown proposal"))?;
        if !matches!(p.source,Source::Master) || !matches!(p.status,ProposalStatus::Parsing|ProposalStatus::Pending|ProposalStatus::NeedsClarification){bail!("only an undecided EXCON proposal can recommend resource limits");}
        self.validate_limits_for_phase(&limits,self.exercise_phase)?;
        let before=p.resource_limit_draft.clone();let after=json!(limits);
        let p=self.proposal_mut(id)?;p.resource_limit_draft=Some(after.clone());p.invalid.retain(|i|!i.starts_with("resource limits:"));
        self.record("resource_limit_draft_reviewed",MASTER,json!({"proposal":id,"before":before,"after":after}));Ok(())
    }

    /// Add shared total/phase feasibility to the simulator's role/inventory checks.
    pub(crate) fn check_limit_batch(&self,except:u64,commands:&[Command],actor:&Actor,verdicts:&mut [Verdict]) {
        let mut used=self.resource_usage_with_commitments(except);
        for (cmd,verdict) in commands.iter().zip(verdicts.iter_mut()) {
            if !verdict.is_ok(){continue;}
            let demand=self.world.resource_requirements(cmd,actor);
            let reason=demand.iter().find_map(|(r,qty)|{
                let cap=self.effective_capacity(r,self.exercise_phase,&self.resource_limits);let committed=used.get(r).copied().unwrap_or(0);
                (committed+*qty as u64>cap).then(||format!("{r}: {committed} already deployed/committed plus {qty} requested exceeds effective total/phase capacity {cap}"))
            });
            if let Some(reason)=reason{*verdict=Verdict::Rejected{reason};}
            else{for (r,qty) in demand{*used.entry(r).or_default()+=qty as u64;}}
        }
    }

    /// Snapshot rollback may restore deployment but cannot silently breach current policy.
    pub(crate) fn validate_restored_limits(&self,pools:&BTreeMap<String,BTreeMap<String,Pool>>)->Result<()> {
        for t in &self.world.scn.resource_types {
            let used=pools.values().filter_map(|p|p.get(&t.id)).map(|p|p.used as u64).sum::<u64>();
            let inventory=pools.values().filter_map(|p|p.get(&t.id)).map(|p|p.total as u64).sum::<u64>();
            let cap=inventory.min(self.resource_limits.total.get(&t.id).copied().map(u64::from).unwrap_or(u64::MAX)).min(self.resource_limits.phases.get(&self.exercise_phase).and_then(|p|p.get(&t.id)).copied().map(u64::from).unwrap_or(u64::MAX));
            if used>cap{bail!("rollback would restore {used} {} above current capacity {cap}; EXCON must review limits first",t.id);}
        }
        Ok(())
    }
}

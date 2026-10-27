//! Immediate physical previews and guarded correction of the latest intervention.
use anyhow::{bail, Result};
use serde_json::{json, Value};
use sim::{Actor, Command, WorldSnapshot};
use crate::{Exercise, Phase, MASTER};

pub(crate) struct Intervention { pub snapshot:WorldSnapshot, pub generation:u64, pub log_seq:u64, pub command:Command }

pub fn physical(cmd:&Command)->bool { matches!(cmd,Command::Ignite {..}|Command::Extinguish {..}|Command::SetWeather {..}|Command::CutFireLine {..}|Command::WaterDrop {..}|Command::SetBuildingStatus {..}) }

impl Exercise {
    pub fn preview_commands(&self,commands:&[Command])->Result<Value> {
        if commands.is_empty()||commands.len()>20 { bail!("preview requires 1–20 commands"); }
        if commands.iter().any(|c|matches!(c,Command::AdvanceTime {..})){bail!("time advance cannot be predicted by an immediate physical preview; use the controlled clock");}
        let mut copy=sim::World::with_options(self.world.scn.clone(),self.world.seed,self.world.fire.options())?;
        copy.restore(&self.world.snapshot())?;
        let before=copy.stats();
        let mut results=Vec::new();
        for cmd in commands {
            let verdict=copy.validate_batch(&self.prior(0),std::slice::from_ref(cmd),&Actor::Master).remove(0);
            let r=if let sim::Verdict::Rejected {reason}=verdict{Err(reason)}else{copy.apply(cmd,&Actor::Master)};
            results.push(json!({"command":cmd,"result":r,"geometry":self.world.command_geometry(cmd)}));
        }
        Ok(json!({"generation":self.generation,"before":before,"after":copy.stats(),"results":results,"warning":"Immediate raster effects only. Fire spread remains stochastic over simulated time; weather does not instantly change the existing perimeter. EXCON intervention uses no role deployment resources. Correction is possible only while paused and before any subsequent exercise change; restoring a checkpoint reseeds future ensemble spread."}))
    }
    pub fn correct_intervention(&mut self,reason:&str)->Result<()> {
        if reason.trim().is_empty()||reason.len()>2000 {bail!("correction requires rationale");}
        if self.clock_control.running||self.phase==Phase::Resolving {bail!("pause simulation before correction");}
        let i=self.intervention.as_ref().ok_or_else(||anyhow::anyhow!("no correctable intervention"))?;
        if i.generation!=self.generation {bail!("exercise has changed since the intervention; use an explicit new correction command to preserve intervening work");}
        self.validate_restored_limits(i.snapshot.resource_pools())?;
        let i=self.intervention.take().unwrap();
        let before=self.world.stats();
        self.world.restore(&i.snapshot)?;
        self.restore_epoch += 1;
        self.clock_control.revision+=1;
        self.record("physical_intervention_corrected",MASTER,json!({"intervention_log_seq":i.log_seq,"command":i.command,"reason":reason,"approved_by":MASTER,"before":before,"after":self.world.stats(),"restore_reseeds_future_spread":true}));
        Ok(())
    }
    pub fn refresh_correction_generation(&mut self) {if let Some(i)=self.intervention.as_mut() {if i.generation+1==self.generation {i.generation=self.generation;}}}
    pub fn correction_view(&self)->Value {
        self.intervention.as_ref().map(|i|json!({"log_seq":i.log_seq,"command":i.command,"available":i.generation==self.generation&&!self.clock_control.running&&self.phase!=Phase::Resolving})).unwrap_or(Value::Null)
    }
}

//! Deployment travel is simulation state, never renderer animation or proposals.
use serde::{Deserialize, Serialize};
use crate::{Actor, Command, World};
use scenario::Pos;

#[derive(Debug, Clone, Serialize)]
pub struct TravelSetting { pub origin:Pos, pub speed_mps:f32 }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum TaskStatus { EnRoute, Working, Completed, Failed, Cancelled }
#[derive(Debug, Clone, Serialize)]
pub struct DeploymentTask {
 pub id:u64, pub role:String, pub allocations:Vec<u64>, pub resource:String, pub qty:u32,
 pub command:Command, pub origin:Pos, pub target:Pos, pub speed_mps:f32,
 pub departed_s:i64, pub arrival_s:i64, pub arrived_s:Option<i64>, pub completed_s:Option<i64>,
 pub status:TaskStatus, pub detail:String,
}
impl DeploymentTask {
 pub fn position(&self,t:i64)->Pos {
  let fraction=if self.arrival_s<=self.departed_s {1.0}else{((t-self.departed_s) as f32/(self.arrival_s-self.departed_s) as f32).clamp(0.0,1.0)};
  [self.origin[0]+(self.target[0]-self.origin[0])*fraction,self.origin[1]+(self.target[1]-self.origin[1])*fraction]
 }
}
impl World {
 pub(crate) fn evacuation_targets(&self,cmd:&Command)->Vec<usize> {match cmd {
  Command::EvacuateZone {zone,..}=>self.zone_code(zone).map(|z|self.buildings_in_zone(&z)).unwrap_or_default(),
  Command::EvacuateBuilding {building,..}=>self.find_building(building).into_iter().collect(), _=>Vec::new(),
 }}
 pub(crate) fn travel_target(&self,cmd:&Command)->Option<Pos> {match cmd {
  Command::CutFireLine {from,to,along,..}=>self.line_endpoints(from,to,along).ok().map(|(a,_)|a),
  Command::WaterDrop {at}=>self.resolve_loc(at).ok(),
  Command::CloseRoad {road}=>self.find_road(road).and_then(|id|self.scn.roads.iter().find(|r|r.id==id)).and_then(|r|r.points.first().copied()),
  Command::EvacuateBuilding {building,..}=>self.find_building(building).map(|i|self.scn.buildings[i].pos),
  Command::EvacuateZone {zone,..}=>self.zone_code(zone).and_then(|z|self.scn.resolve_place(&z)),
  _=>None,
 }}
 pub(crate) fn dispatch(&mut self,cmd:&Command,role:&str,allocations:Vec<u64>)->Option<String> {
  let target=self.travel_target(cmd)?;
  let a=self.d.allocations.iter().find(|a|allocations.contains(&a.id))?;
  let setting=self.d.travel_settings.get(role)?.get(&a.resource)?;
  let distance=((target[0]-setting.origin[0]).powi(2)+(target[1]-setting.origin[1]).powi(2)).sqrt();
  let departed_s=self.time_s();let arrival_s=departed_s+(distance/setting.speed_mps).ceil().max(1.0) as i64;
  let task=DeploymentTask {id:a.id,role:role.into(),allocations,resource:a.resource.clone(),qty:a.qty,command:cmd.clone(),origin:setting.origin,target,speed_mps:setting.speed_mps,departed_s,arrival_s,arrived_s:None,completed_s:None,status:TaskStatus::EnRoute,detail:format!("Dispatched; arrival at t+{arrival_s}s; task effect waits for arrival")};
  let detail=task.detail.clone();
  self.d.tasks.push(task);
  self.event("deployment_dispatched",format!("{role}: {detail}"),self.scn.zone_of(target));
  Some(detail)
 }
 pub(crate) fn step_tasks(&mut self) {
  let t=self.time_s();
  let due=self.d.tasks.iter().enumerate().filter(|(_,task)|task.status==TaskStatus::EnRoute&&task.arrival_s<=t).map(|(i,_)|i).collect::<Vec<_>>();
  for i in due {
   let task=self.d.tasks[i].clone();
   // Re-check physical feasibility at actual arrival, without charging the
   // already allocated units again. Role timing/provenance remain preserved.
   let mut pools=self.d.pools.clone();
   let result=match self.check_inner(&task.command,&Actor::Master,&mut pools) {
    Ok(_)=>self.apply_effect(&task.command,&Actor::Role(task.role.clone())),Err(e)=>Err(e),
   };
   let evacuation=!self.evacuation_targets(&task.command).is_empty();
   let task=&mut self.d.tasks[i];task.arrived_s=Some(t);
   match result {Ok(notes)=>{task.status=if evacuation {TaskStatus::Working}else{TaskStatus::Completed};task.detail=notes.join("; ");if !evacuation {task.completed_s=Some(t);}},Err(reason)=>{task.status=TaskStatus::Failed;task.completed_s=Some(t);task.detail=reason;}}
   let detail=task.detail.clone();let role=task.role.clone();
   self.event("deployment_arrived",format!("{role} task #{} arrived: {detail}",self.d.tasks[i].id),self.scn.zone_of(self.d.tasks[i].target));
  }
  let complete=self.d.tasks.iter().enumerate().filter(|(_,task)|task.status==TaskStatus::Working && self.evacuation_targets(&task.command).iter().all(|i|self.d.buildings[*i].evac_done_at.is_none())).map(|(i,_)|i).collect::<Vec<_>>();
  for i in complete {let task=&mut self.d.tasks[i];task.status=TaskStatus::Completed;task.completed_s=Some(t);task.detail="Evacuation completed; units remain deployed until EXCON confirms return".into();let id=task.id;self.event("deployment_completed",format!("Evacuation task #{id} completed"),None);}
 }
 pub fn task_active_qty(&self,task:&DeploymentTask)->u32 {self.d.allocations.iter().filter(|a|task.allocations.contains(&a.id)).map(|a|a.qty.saturating_sub(a.returned)).sum()}
 pub fn deployment_view(&self)->serde_json::Value {serde_json::json!(self.d.tasks.iter().map(|task|serde_json::json!({"task":task,"position_m":task.position(self.time_s()),"active_qty":self.task_active_qty(task)})).collect::<Vec<_>>())}
}

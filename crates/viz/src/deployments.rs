//! Draw only real simulation deployments. The clock, route and status come from
//! World; the renderer neither creates deployments nor advances their travel.
use bevy::prelude::*;
use sim::TaskStatus;
use crate::{Scn,Snapshot};
use crate::frame::to_bevy;

#[derive(Component)]
pub struct DeploymentMarker(pub u64);
#[derive(Resource)]
pub struct DeploymentPalette {ground:Handle<Mesh>,air:Handle<Mesh>,enroute:Handle<StandardMaterial>,working:Handle<StandardMaterial>,completed:Handle<StandardMaterial>,failed:Handle<StandardMaterial>}
pub fn setup(mut commands:Commands,mut meshes:ResMut<Assets<Mesh>>,mut materials:ResMut<Assets<StandardMaterial>>) {
 let mut material=|c|materials.add(StandardMaterial {base_color:c,emissive:c.to_linear()*0.3,..default()});
 commands.insert_resource(DeploymentPalette {ground:meshes.add(Cuboid::new(32.0,18.0,52.0)),air:meshes.add(Cuboid::new(80.0,12.0,35.0)),enroute:material(Color::srgb(1.0,0.65,0.1)),working:material(Color::srgb(0.1,0.9,1.0)),completed:material(Color::srgb(0.2,1.0,0.35)),failed:material(Color::srgb(1.0,0.1,0.15))});
}
pub fn update(mut commands:Commands,scn:Res<Scn>,snap:Res<Snapshot>,palette:Res<DeploymentPalette>,mut markers:Query<(Entity,&DeploymentMarker,&mut Transform,&mut Handle<StandardMaterial>)>,mut gizmos:Gizmos) {
 let visible=snap.deployments.iter().filter(|(task,qty)|*qty>0 && task.status!=TaskStatus::Cancelled).collect::<Vec<_>>();
 for (entity,marker,mut transform,mut material) in &mut markers {
  if let Some((task,_))=visible.iter().find(|(task,_)|task.id==marker.0) {set(&scn,task,snap.time_s,&palette,&mut transform,&mut material);}else{commands.entity(entity).despawn_recursive();}
 }
 for (task,_) in visible {
  let airborne=task.resource=="air_tankers";
  let mut transform=Transform::default();let mut material=palette.enroute.clone();set(&scn,task,snap.time_s,&palette,&mut transform,&mut material);
  if !markers.iter().any(|(_,m,_,_)|m.0==task.id) {commands.spawn((DeploymentMarker(task.id),PbrBundle {mesh:if airborne {palette.air.clone()}else{palette.ground.clone()},material,transform,..default()}));}
  if task.status==TaskStatus::EnRoute {gizmos.line(to_bevy(&scn.0,task.origin,if airborne {120.0}else{12.0}),to_bevy(&scn.0,task.target,if airborne {120.0}else{12.0}),Color::srgba(1.0,0.65,0.1,0.5));}
 }
}
fn set(scn:&Scn,task:&sim::DeploymentTask,t:i64,palette:&DeploymentPalette,transform:&mut Transform,material:&mut Handle<StandardMaterial>) {
 let lift=if task.resource=="air_tankers" {120.0}else{12.0};
 transform.translation=to_bevy(&scn.0,task.position(t),lift);
 let direction=to_bevy(&scn.0,task.target,lift)-to_bevy(&scn.0,task.origin,lift);
 if direction.length_squared()>0.01 {transform.rotation=Quat::from_rotation_y(direction.x.atan2(direction.z));}
 *material=match task.status {TaskStatus::EnRoute=>palette.enroute.clone(),TaskStatus::Working=>palette.working.clone(),TaskStatus::Completed=>palette.completed.clone(),_=>palette.failed.clone()};
}

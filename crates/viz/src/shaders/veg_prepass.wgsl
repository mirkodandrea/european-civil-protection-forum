// Depth, normal and shadow pass vertex stage of the vegetation: Bevy 0.14's
// `prepass.wgsl` vertex without morph targets and skinning, plus the per-plant
// fire state (see `veg_vertex.wgsl`), so a plant that has collapsed leaves no
// shadow. The vegetation material has no uv and no tangent.
#import bevy_pbr::{
    mesh_functions,
    prepass_io::VertexOutput,
    view_transformations::position_world_to_clip,
}
#import rocca::veg_state::plant_apply

struct VegVertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
#ifdef NORMAL_PREPASS_OR_DEFERRED_PREPASS
    @location(3) normal: vec3<f32>,
#endif
#ifdef VERTEX_COLORS
    @location(7) color: vec4<f32>,
#endif
    @location(14) plant: u32,
}

@vertex
fn vertex(vertex: VegVertex) -> VertexOutput {
    var out: VertexOutput;
    var position = vertex.position;
#ifdef VERTEX_COLORS
    let planted = plant_apply(vertex.plant, vertex.position, vertex.color);
    position = planted.position;
    out.color = planted.color;
#endif
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
#ifdef DEPTH_CLAMP_ORTHO
    out.clip_position_unclamped = out.position;
    out.position.z = min(out.position.z, 1.0);
#endif
#ifdef NORMAL_PREPASS_OR_DEFERRED_PREPASS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
#endif
#ifdef MOTION_VECTOR_PREPASS
    // the plants do not move, collapsed or not
    let prev_model = mesh_functions::get_previous_world_from_local(vertex.instance_index);
    out.previous_world_position = mesh_functions::mesh_position_local_to_world(prev_model, vec4<f32>(position, 1.0));
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
    return out;
}

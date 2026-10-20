// Smoke puffs: an unlit, alpha-blended billboard.
//
// The same picture the unlit `StandardMaterial` used to draw (vertex colour x
// puff texture, fogged), minus everything a puff does not need: no PbrInput,
// no lights, no clustering, no material uniform reads. Hundreds of big,
// overlapping puffs make this the most fill-bound thing in the scene, so every
// instruction per pixel counts.
//
// Fog is the costly part per pixel (a light loop and a pow) and it is smooth
// across a puff, so the CPU evaluates it once per puff (`PuffFog` in
// fire_view.rs). Fog is affine in the colour it fogs: the gain is already in
// the vertex colour, the additive part arrives in the normal slot.

#import bevy_pbr::mesh_functions::{get_world_from_local, mesh_position_local_to_world}
#import bevy_pbr::view_transformations::position_world_to_clip
#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping::tone_mapping
#import bevy_pbr::mesh_view_bindings::view
#endif

@group(2) @binding(0) var puff_texture: texture_2d<f32>;
@group(2) @binding(1) var puff_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) fog_offset: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(5) color: vec4<f32>,
}

struct PuffOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) fog_offset: vec3<f32>,
}

@vertex
fn vertex(v: Vertex) -> PuffOut {
    var out: PuffOut;
    let world = mesh_position_local_to_world(
        get_world_from_local(v.instance_index),
        vec4<f32>(v.position, 1.0),
    ).xyz;
    out.clip = position_world_to_clip(world);
    out.uv = v.uv;
    out.color = v.color;
    out.fog_offset = v.fog_offset;
    return out;
}

@fragment
fn fragment(in: PuffOut) -> @location(0) vec4<f32> {
    let tex = textureSample(puff_texture, puff_sampler, in.uv);
    let a = in.color.a * tex.a;
    // Nothing visible left: skip the blend (the texture's rim is mostly this).
    if a < 0.002 {
        discard;
    }
    var col = vec4<f32>(in.color.rgb * tex.rgb + in.fog_offset, a);
#ifdef TONEMAP_IN_SHADER
    col = tone_mapping(col, view.color_grading);
#endif
    return col;
}

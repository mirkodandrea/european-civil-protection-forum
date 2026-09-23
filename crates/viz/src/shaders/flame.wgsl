// Flame tongues (additive). The CPU side (flames.rs) decides where each tongue
// stands, how tall it is, how far the wind tilts it and how hot it is; this
// shader makes the quad burn: an animated domain warp of the tongue texture
// (strong at the roiling base, calm at the tip), a hot-core colour ramp, and a
// per-tongue flicker seeded by world position so neighbours never breathe in
// lockstep. Vertex alpha scales the whole tongue (the uncertainty halo uses
// dim, low-alpha tongues).

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::globals

@group(2) @binding(0) var tongue_texture: texture_2d<f32>;
@group(2) @binding(1) var tongue_sampler: sampler;

fn hash21(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash21(i), hash21(i + vec2<f32>(1.0, 0.0)), u.x),
               mix(hash21(i + vec2<f32>(0.0, 1.0)), hash21(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}

fn fbm(p: vec2<f32>) -> f32 {
    return 0.55 * value_noise(p) + 0.3 * value_noise(p * 2.03 + 7.1) + 0.15 * value_noise(p * 4.1 - 3.7);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let t = globals.time;
    let seed = in.world_position.x * 0.131 + in.world_position.z * 0.077;
    var uv = in.uv;
    // uv.y = 1 at the base, 0 at the tip.
    let base = uv.y;
    let amp = 0.16 * (0.25 + 0.75 * base);
    let n1 = fbm(vec2<f32>(uv.x * 4.0 + seed, uv.y * 3.0 - t * 3.4));
    let n2 = fbm(vec2<f32>(uv.x * 7.0 - seed, uv.y * 5.0 - t * 6.1 + 4.0));
    uv.x += (n1 - 0.5) * amp;
    uv.y += (n2 - 0.5) * amp * 0.5;
    let tex = textureSample(tongue_texture, tongue_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)));
    // Hot core: whiter at the base and the centre line, deep orange at the edges and tip.
    let core = tex.r * (0.45 + 0.55 * base);
    let ramp = mix(vec3<f32>(1.0, 0.32, 0.05), vec3<f32>(1.0, 0.85, 0.55), clamp(core, 0.0, 1.0));
    let flicker = 0.78 + 0.34 * value_noise(vec2<f32>(t * 10.0 + seed * 7.0, seed * 3.0));
    let a = tex.a * in.color.a;
    return vec4<f32>(in.color.rgb * ramp * flicker * a, a);
}

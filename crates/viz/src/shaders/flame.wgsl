// Fragment shader for flame billboards.
//
// The CPU side (fire_view::update_flames) already decides *where* a tongue
// stands, how tall it is and how hot: this shader is only responsible for
// making one quad look like fire instead of a lit sprite. It domain-warps
// the texture lookup with a small animated noise field and adds a fast
// per-instance flicker, both driven by `globals.time` and the billboard's
// own world position (so tongues a metre apart do not flicker in lockstep).

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::globals
#import rocca::noise::{hash21, value_noise}

@group(2) @binding(0) var base_texture: texture_2d<f32>;
@group(2) @binding(1) var base_sampler: sampler;
@group(2) @binding(2) var<uniform> mode: vec4<f32>;

fn fbm2(p_in: vec2<f32>) -> f32 {
    var p = p_in;
    var a = 0.5;
    var s = 0.0;
    for (var i = 0; i < 3; i++) {
        s += a * value_noise(p);
        p = p * 2.13 + vec2<f32>(5.1, 1.7);
        a *= 0.5;
    }
    return s / 0.875;
}

// Blackbody-ish ramp: deep red tip -> orange -> yellow -> white-hot core.
fn flame_ramp(h: f32) -> vec3<f32> {
    let c0 = vec3<f32>(0.9, 0.10, 0.02);
    let c1 = vec3<f32>(2.4, 0.55, 0.06);
    let c2 = vec3<f32>(3.4, 1.7, 0.25);
    let c3 = vec3<f32>(4.2, 3.4, 1.8);
    let a = clamp(h * 3.0, 0.0, 1.0);
    let b = clamp(h * 3.0 - 1.0, 0.0, 1.0);
    let c = clamp(h * 3.0 - 2.0, 0.0, 1.0);
    return mix(mix(mix(c0, c1, a), c2, b), c3, c);
}

// Glowing ember temperature, h = 1 just thrown ... 0 spent: white-yellow,
// orange, dull red, then a dark cinder.
fn ember_ramp(h: f32) -> vec3<f32> {
    let spent = vec3<f32>(0.22, 0.025, 0.0);
    let red = vec3<f32>(1.5, 0.20, 0.02);
    let orange = vec3<f32>(3.0, 1.05, 0.12);
    let white = vec3<f32>(4.2, 2.9, 1.3);
    let a = smoothstep(0.0, 0.30, h);
    let b = smoothstep(0.30, 0.65, h);
    let c = smoothstep(0.65, 1.0, h);
    return mix(mix(mix(spent, red, a), orange, b), white, c);
}

// An ember or a landing flash (`fire_view::update_flames`): an oval quad
// (uv 0..1) with the round shape drawn per pixel. Vertex colour:
// r = share of its flight, g = phase, b = gain (negative: a landing flash).
fn ember(in: VertexOutput, t: f32) -> vec4<f32> {
    let k = clamp(in.color.r, 0.0, 1.0);
    let ph = in.color.g;
    let flash = in.color.b < 0.0;
    let gain = abs(in.color.b);
    let x = in.uv.x * 2.0 - 1.0;
    let y = in.uv.y * 2.0 - 1.0;

    if flash {
        let f2 = (1.0 - clamp(x * x + y * y, 0.0, 1.0));
        let f4 = f2 * f2;
        return vec4<f32>(vec3<f32>(4.0, 2.2, 0.9) * gain * (f4 * f4 + 0.25 * f4), 0.0);
    }

    // A round glowing dot, drawn per pixel: a bright core, a soft body and a
    // halo, all exactly zero before the quad's edge so no square shows.
    let r = clamp(length(vec2<f32>(x, y)), 0.0, 1.0);
    let body = 1.0 - smoothstep(0.45, 0.95, r);
    let core = 1.0 - smoothstep(0.0, 0.40, r);
    let shape = 0.9 * body + 1.2 * core * core + 0.25 * (1.0 - r) * (1.0 - r);

    // It cools over its flight and flickers as it tumbles.
    let heat = 1.0 - pow(k, 0.7);
    let flick = 0.6 + 0.4 * sin(t * (13.0 + ph * 23.0) + ph * 50.0);
    let life = smoothstep(1.0, 0.78, k);
    let col = ember_ramp(heat) * shape * gain * flick * life;
    // Alpha 0: Bevy's Add is premultiplied (src + dst * (1 - alpha)), so any
    // alpha would darken what is behind the dot into a dark rim.
    return vec4<f32>(col, 0.0);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let t = globals.time;
    if mode.x > 1.5 {
        return ember(in, t);
    }
    if mode.x < 0.5 {
        let s0 = in.world_position.x * 0.13 + in.world_position.z * 0.071;
        let tx = textureSample(base_texture, base_sampler, in.uv);
        let fl = 0.80 + 0.35 * value_noise(vec2<f32>(t * 11.0 + s0 * 7.0, s0 * 3.0));
        var c0 = in.color * tx * fl;
        c0.a = tx.a * in.color.a;
        return c0;
    }
    // A per-tongue phase from its own world position, so two billboards
    // spawned on the same frame do not breathe in unison.
    let seed = in.world_position.x * 0.13 + in.world_position.z * 0.071;

    // uv.y: 0 at the tip, 1 at the foot (see QuadBuilder::billboard).
    var uv = in.uv;
    let foot = uv.y;

    // Fire rises: noise scrolls upward fast, and the sideways warp grows
    // toward the tip so the flame licks and curls instead of standing still.
    let rise = vec2<f32>(uv.x * 4.0 + seed, uv.y * 3.5 + t * 3.4);
    let warp = fbm2(rise) - 0.5;
    // Lean: the tip streams sideways on a slow wave, the foot stays put, so
    // the tongue bends and whips instead of bobbing up and down.
    let lean = (value_noise(vec2<f32>(t * 1.7 + seed * 5.0, 3.0)) - 0.5) * 0.9;
    uv.x += warp * (0.12 + 0.38 * (1.0 - foot)) + lean * (1.0 - foot) * (1.0 - foot);
    uv.y += (value_noise(vec2<f32>(uv.x * 6.0 - seed, uv.y * 5.0 + t * 4.5)) - 0.5) * 0.06;

    let tex = textureSample(base_texture, base_sampler, uv);

    // Erode the silhouette with rising noise: tongues break into flickering
    // wisps toward the tip, but the foot stays solid.
    let erode = fbm2(vec2<f32>(uv.x * 7.0 + seed * 2.0, uv.y * 6.0 + t * 3.8));
    // Hard guarantee that nothing is drawn at the quad's top edge.
    let tipfade = smoothstep(0.0, 0.22, uv.y) * smoothstep(0.0, 0.06, uv.x) * smoothstep(1.0, 0.94, uv.x);
    let body = tipfade * tex.a * smoothstep(0.0, 0.55, tex.a * 1.6 + (erode - 0.5) * (0.9 - 0.5 * foot));

    // Heat: hot at the foot and core, cooling toward the tip and edges.
    let flicker = 0.85 + 0.30 * value_noise(vec2<f32>(t * 9.0 + seed * 7.0, seed * 3.0));
    let heat = clamp(body * (0.35 + 0.85 * foot) * flicker + (erode - 0.5) * 0.25, 0.0, 1.0);

    // in.color carries the CPU's brightness (cell intensity); use it as a
    // gain on the ramp, not as the colour itself.
    let gain = clamp(in.color.r * 0.30, 0.55, 1.6);
    // TTX: Bevy Add expects premultiplied radiance. Mask RGB as well as
    // alpha, otherwise the ribbon remains a luminous solid triangle.
    let coverage = body * in.color.a;
    var col = vec4<f32>(flame_ramp(heat) * gain * flicker * coverage, 0.0);
    return col;
}

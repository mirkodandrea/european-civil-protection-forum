// Fragment shader for the ground overlay.
//
// The CPU side (fire_view::update_overlay) rebuilds this mesh's vertex
// colours at most every 150 ms — plenty for the burn perimeter, which does
// not move that fast, but not enough on its own to look "alive" at 1x speed.
// `sample_color` flags a hot, glowing fragment by pushing its red channel
// above 1.0 (the same convention the flame billboards use, so the two never
// disagree about what is currently burning); this shader only adds motion to
// that already-decided glow. Every other layer (Intensity, Arrival, Hazard)
// never sets red above 1.0, so they pass through unanimated.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::globals
#import rocca::noise::{hash21, value_noise}
#import rocca::scene_light::{scene_shade, apply_scene_fog}

@group(2) @binding(0) var road_tex: texture_2d<f32>;
@group(2) @binding(1) var road_samp: sampler;
@group(2) @binding(2) var<uniform> mask_rect: vec4<f32>;

// Keep in step with `ground_mask.rs` and `terrain.wgsl`.
const RANGE_M: f32 = 12.0;

// Fractal Brownian motion, domain-warped: the warp is what turns blobby
// noise into the tendrilled, branching texture of a real fire front.
fn fbm(p_in: vec2<f32>) -> f32 {
    var p = p_in;
    var a = 0.5;
    var s = 0.0;
    for (var i = 0; i < 5; i++) {
        s += a * value_noise(p);
        p = p * 2.07 + vec2<f32>(17.3, 9.1);
        a *= 0.5;
    }
    return s / 0.96875;
}

fn fire_ramp(h: f32) -> vec3<f32> {
    // ember red -> orange -> yellow -> near white, h in 0..1
    let c0 = vec3<f32>(0.80, 0.08, 0.02);
    let c1 = vec3<f32>(2.50, 0.60, 0.06);
    let c2 = vec3<f32>(3.60, 1.80, 0.28);
    let c3 = vec3<f32>(4.40, 3.50, 1.60);
    let a = clamp(h * 3.0, 0.0, 1.0);
    let b = clamp(h * 3.0 - 1.0, 0.0, 1.0);
    let c = clamp(h * 3.0 - 2.0, 0.0, 1.0);
    return mix(mix(mix(c0, c1, a), c2, b), c3, c);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var col = in.color;
    // `w` fades the fractal treatment in smoothly over the glow gradient so
    // there is no seam where the CPU colour crosses the hot threshold.
    let w = smoothstep(0.35, 1.0, col.r);
    if w > 0.0 {
        let t = globals.time;
        let wp = in.world_position.xz;
        // Heat decoded from the CPU glow (r = char + 2.4 * glow).
        let heat = clamp((col.r - 0.3) / 2.4, 0.0, 1.0);

        // Domain warp, slowly advected by the wind-ish drift.
        let q = wp * 0.05 + vec2<f32>(t * 0.35, -t * 0.28);
        let warp = vec2<f32>(fbm(q + 3.7), fbm(q + 11.2)) - 0.5;
        let p = wp * 0.11 + warp * 2.6;
        let n = fbm(p + vec2<f32>(t * 0.6, -t * 0.45));
        // Fine, fast crackle on top: individual embers winking.
        let crackle = value_noise(wp * 0.9 + vec2<f32>(t * 3.0, t * 2.1));

        // Hot cells are where local heat beats the noise threshold: the
        // front breaks into patches and fingers, with a ragged border.
        // Slow breathing of the whole front, offset by position so it rolls
        // along the line instead of pulsing in unison.
        let breath = 0.5 + 0.5 * sin(t * 2.2 + wp.x * 0.035 + wp.y * 0.027);
        let local = heat * 1.75 - (1.0 - n) * 0.70 + (crackle - 0.5) * 0.30 + breath * 0.10;
        let hot = smoothstep(0.0, 0.18, local);

        let fire = fire_ramp(clamp(local, 0.0, 1.0)) * (0.85 + 0.55 * crackle);
        // What the cooler gaps look like: dark char with a faint red glint.
        let char_col = vec3<f32>(0.06, 0.03, 0.025) + vec3<f32>(0.9, 0.12, 0.0) * (1.0 - hot) * heat * n;
        let fractal = mix(char_col, fire, hot);
        let rgb = mix(col.rgb, fractal, w);

        // Ragged outer edge: alpha is a smooth coverage ramp on the CPU.
        let edge = smoothstep(0.12, 0.5, col.a + (n - 0.5) * 0.55);
        let alpha = mix(col.a, col.a * edge, w);
        col = vec4<f32>(rgb, alpha);
    }

    // Cold scar: ragged edge and ash grain at pixel scale, and the roads
    // showing through it. The vertex lattice is 5 m or coarser, so without
    // this the scar's border is a visible staircase and roads vanish under it.
    let wpc = in.world_position.xz;
    let cold = 1.0 - w;
    if cold > 0.0 && col.a > 0.0 {
        // Fractal border: a domain-warped fbm (large lobes down to ~1 m
        // fingers) moves the coverage threshold, so the burn edge is a
        // self-similar coastline with peninsulas and unburned islands
        // instead of a smooth blob. The push is strongest where coverage
        // is low or middling (the border band) and fades inside the scar.
        let warp = vec2<f32>(value_noise(wpc * 0.06 + 7.1), value_noise(wpc * 0.06 + 23.9)) - 0.5;
        let fr = fbm(wpc * 0.13 + warp * 3.2);
        let fine = value_noise(wpc * 1.9 + 5.3);
        let n2 = 0.75 * fr + 0.25 * fine;
        let band = 1.0 - smoothstep(0.35, 0.95, col.a);
        let m = col.a + (n2 - 0.5) * (0.55 + 0.85 * band);
        let ragged = smoothstep(0.22, 0.34, m);
        // Scorched fringe: browned, half-burnt vegetation just inside the
        // border, darkening to ash towards the interior.
        let fringe = smoothstep(0.22, 0.30, m) * (1.0 - smoothstep(0.30, 0.62, m));
        let scorch = mix(vec3<f32>(1.0), vec3<f32>(1.9, 1.15, 0.55), fringe * 0.7);
        // Burnt ground as a material, not a flat tint: the CPU colour is a
        // near-neutral dark; here it is pushed between warm brown soil,
        // cool grey ash and near-black charcoal by noise at three scales,
        // then broken up by fine grit and hairline cracks.
        let big = fbm(wpc * 0.035 + 31.0);
        let mid = fbm(wpc * 0.16 + warp * 1.5 + 9.0);
        let grit = value_noise(wpc * 4.3) * 0.6 + value_noise(wpc * 11.0 + 2.0) * 0.4;
        let lum = dot(col.rgb, vec3<f32>(0.33, 0.45, 0.22));
        let brown = vec3<f32>(1.35, 0.95, 0.68);
        let grey = vec3<f32>(0.95, 1.0, 1.08);
        let hue = smoothstep(0.30, 0.70, big * 0.65 + mid * 0.35);
        var tone = mix(brown, grey, hue);
        // Ash drifts: pale grey patches, charcoal pockets, both patchy.
        let ash = smoothstep(0.58, 0.82, mid) * (1.0 - fringe);
        let coal = smoothstep(0.62, 0.20, mid * 0.7 + grit * 0.3);
        var base = vec3<f32>(lum) * tone * (0.9 + 0.7 * (mid - 0.5));
        base = mix(base, vec3<f32>(0.30, 0.285, 0.27) * (0.8 + 0.4 * grit), ash * 0.65);
        base = mix(base, base * 0.35, coal * 0.6);
        // Hairline cracks and pits: dark ridges where two noise fields cross.
        let cr = abs(value_noise(wpc * 0.9 + warp * 6.0) - 0.5);
        let crack = 1.0 - smoothstep(0.0, 0.035, cr);
        base *= 1.0 - 0.45 * crack;
        // Matte, gritty finish: high-frequency speckle, no uniform sheen.
        base *= 0.82 + 0.36 * grit;
        col = vec4<f32>(mix(col.rgb, base, cold) * scorch, mix(col.a, col.a * ragged, cold));
    }
    let road_d = (textureSample(road_tex, road_samp, wpc * mask_rect.xy + vec2<f32>(0.5)).rg * 2.0 - 1.0) * RANGE_M;
    let mpp = max(length(dpdx(wpc)), length(dpdy(wpc)));
    let aa = max(0.5 * mpp, 0.25);
    let on_road = max(
        1.0 - smoothstep(max(0.0, 0.8 * mpp - 4.8) - aa, max(0.0, 0.8 * mpp - 4.8) + aa, road_d.x),
        1.0 - smoothstep(max(0.0, 0.8 * mpp - 1.6) - aa, max(0.0, 0.8 * mpp - 1.6) + aa, road_d.y),
    );
    col.a = col.a * (1.0 - 0.72 * on_road * cold);

    // Scene light on the cold scar, so it shades and shadows like the ground
    // under it: the facet normal of the lattice, roughened by a bump of the
    // grit noise so the ash catches the sun grain by grain. The glowing part
    // is a light source and is left as it is. Fog applies to both.
    let facet = cross(dpdx(in.world_position.xyz), dpdy(in.world_position.xyz));
    var nrm = normalize(facet);
    if nrm.y < 0.0 {
        nrm = -nrm;
    }
    let e = 0.35;
    let h0 = value_noise(wpc * 1.3) + 0.5 * value_noise(wpc * 3.1);
    let hx = value_noise((wpc + vec2<f32>(e, 0.0)) * 1.3) + 0.5 * value_noise((wpc + vec2<f32>(e, 0.0)) * 3.1);
    let hz = value_noise((wpc + vec2<f32>(0.0, e)) * 1.3) + 0.5 * value_noise((wpc + vec2<f32>(0.0, e)) * 3.1);
    nrm = normalize(nrm + vec3<f32>(h0 - hx, 0.0, h0 - hz) * 0.9);
    let shade = scene_shade(in.world_position.xyz, nrm, in.position);
    let lit_rgb = mix(col.rgb, col.rgb * shade * 0.55, cold);
    col = vec4<f32>(apply_scene_fog(vec4<f32>(lit_rgb, col.a), in.world_position.xyz).rgb, col.a);
    return col;
}

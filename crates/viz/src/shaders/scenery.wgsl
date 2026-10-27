// Surface functions shared by the scenery materials (`materials/`): the
// island's limestone, read by the terrain's rock faces and by the rock meshes
// off the terrain, and the tactical view's greying of the backdrop.
#define_import_path rocca::scenery

#import rocca::noise::value_noise

// Limestone strata colour from the exact height of the pixel: pale and ochre
// beds with a thin grey-iron seam, dipping with noise. Drawn per pixel because
// the same bands per vertex (5 m grid, 14 m beds) alias into vertical streaks
// on a steep face.
fn limestone(p: vec3<f32>) -> vec3<f32> {
    let pale = vec3<f32>(0.64, 0.60, 0.52);
    let ochre = vec3<f32>(0.62, 0.46, 0.29);
    let grey = vec3<f32>(0.50, 0.49, 0.46);
    let dip = value_noise(p.xz / 160.0 + 5.0) * 26.0 + value_noise(p.xz / 38.0 + 9.0) * 5.0;
    let t = fract((p.y + dip) / 14.0);
    var c = pale;
    c = mix(c, ochre, smoothstep(0.30, 0.40, t) - smoothstep(0.62, 0.72, t));
    c = mix(c, grey, smoothstep(0.88, 0.94, t) - smoothstep(0.96, 1.0, t));
    return c;
}

// Rock-face strata: the bed coordinate (a bed is ~1.8 m) and the horizontal
// tangent of the face, shared by the albedo and the normal.
fn rock_bed(p: vec3<f32>, nrm: vec3<f32>) -> vec3<f32> {
    let nh = normalize(nrm.xz + vec2<f32>(1.0e-4));
    let along = dot(p.xz, vec2<f32>(-nh.y, nh.x));
    let yy = p.y + value_noise(vec2<f32>(along * 0.04, p.y * 0.05) + 3.0) * 7.0;
    let bed = value_noise(vec2<f32>(along * 0.10, yy * 0.8));
    return vec3<f32>(along, yy, yy * 0.55 + bed * 0.7);
}

// Fluting of a bedded face: each bed shelves outward and overhangs a little,
// so the sun finds a ledge and a shadow line in every one. Added to the world
// normal of the terrain on steep ground; fades with pixel size.
fn rock_bump(p: vec3<f32>, nrm: vec3<f32>) -> vec3<f32> {
    let steep = (1.0 - smoothstep(0.72, 0.90, nrm.y));
    let mpp = max(length(dpdx(p)), length(dpdy(p)));
    let fine = 1.0 - smoothstep(1.2, 4.0, mpp);
    if steep < 0.01 || fine < 0.01 {
        return vec3<f32>(0.0);
    }
    let b = rock_bed(p, nrm);
    let f = fract(b.z);
    let nh = normalize(nrm.xz + vec2<f32>(1.0e-4));
    let shelf = (smoothstep(0.0, 0.7, f) - smoothstep(0.7, 1.0, f)) - 0.5;
    let flute = value_noise(vec2<f32>(b.x * 0.9, b.y * 0.3)) - 0.5;
    return vec3<f32>(nh.x * flute * 0.35, shelf * 0.7, nh.y * flute * 0.35) * steep * fine;
}

// Tactical view (`tactical.rs`): the backdrop recedes toward grey so houses,
// units and the interface carry the colour; `amount` < 0 pushes the colour
// away from grey (more saturated). Applied after the fog, whose blue would
// otherwise keep the far ground tinted.
fn tactical_grey(c: vec4<f32>, amount: f32) -> vec4<f32> {
    let luma = dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    return vec4<f32>(max(mix(c.rgb, vec3<f32>(luma) * 0.9, amount), vec3<f32>(0.0)), c.a);
}

// The scene's own light and fog, for the materials that draw on the ground
// themselves (the burn scar, the hazard tape). `StandardMaterial` gets both
// for free; a custom `Material` does not, and without them these surfaces
// stay flat and sunlit while the terrain around them is shaded, shadowed and
// hazed. Imported as `rocca::scene_light` (registered in `shader_lib.rs`).
//
// Map signs (flat, reach_zone, route_line, forecast_zone) deliberately do NOT
// use this: they are interface and must stay legible in shadow and distance.

#define_import_path rocca::scene_light

#import bevy_pbr::mesh_view_bindings::{view, fog, lights}
#import bevy_pbr::mesh_view_types
#import bevy_pbr::fog as scene_fog
#import bevy_pbr::shadows

// Brightness multiplier for a surface with unit normal `n` at `world_pos`:
// ambient plus the sun (Lambert, with the cascaded shadow map). `frag` is the
// `@builtin(position)` of the fragment. Multiply an albedo by it.
fn scene_shade(world_pos: vec3<f32>, n: vec3<f32>, frag: vec4<f32>) -> vec3<f32> {
    var lit = lights.ambient_color.rgb * view.exposure;
    if lights.n_directional_lights > 0u {
        let light = lights.directional_lights[0];
        let ndl = max(dot(n, light.direction_to_light), 0.0);
        let view_z = dot(vec4<f32>(
            view.view_from_world[0].z,
            view.view_from_world[1].z,
            view.view_from_world[2].z,
            view.view_from_world[3].z,
        ), vec4<f32>(world_pos, 1.0));
        var shadow = 1.0;
        if (light.flags & mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u {
            shadow = shadows::fetch_directional_shadow(0u, vec4<f32>(world_pos, 1.0), n, view_z);
        }
        lit += light.color.rgb * view.exposure * ndl * shadow * 0.3183;
    }
    return lit;
}

// The scene's distance fog, applied by hand: `apply_fog` transcribed, because
// importing `pbr_functions` would pull in `StandardMaterial`'s own bindings.
fn apply_scene_fog(input_color: vec4<f32>, world_position: vec3<f32>) -> vec4<f32> {
    let view_to_world = world_position - view.world_position.xyz;
    let distance = length(view_to_world);

    var scattering = vec3<f32>(0.0);
    if fog.directional_light_color.a > 0.0 {
        let dir = view_to_world / max(distance, 1e-3);
        for (var i: u32 = 0u; i < lights.n_directional_lights; i = i + 1u) {
            let light = lights.directional_lights[i];
            scattering += pow(
                max(dot(dir, light.direction_to_light), 0.0),
                fog.directional_light_exponent,
            ) * light.color.rgb * view.exposure;
        }
    }

    if fog.mode == mesh_view_types::FOG_MODE_LINEAR {
        return scene_fog::linear_fog(fog, input_color, distance, scattering);
    } else if fog.mode == mesh_view_types::FOG_MODE_EXPONENTIAL {
        return scene_fog::exponential_fog(fog, input_color, distance, scattering);
    } else if fog.mode == mesh_view_types::FOG_MODE_EXPONENTIAL_SQUARED {
        return scene_fog::exponential_squared_fog(fog, input_color, distance, scattering);
    } else if fog.mode == mesh_view_types::FOG_MODE_ATMOSPHERIC {
        return scene_fog::atmospheric_fog(fog, input_color, distance, scattering);
    }
    return input_color;
}

// Chrome pinball ball. The ball is a flat 2D disc, but we reconstruct a
// hemisphere normal across it and shade it like a polished steel sphere.
//
// Two reflections combine, the way a real chrome ball reads:
//   - The distant environment (an equirectangular map: the table's ball image
//     when it ships one, else a neutral studio gradient). This is fixed in space
//     and does not move with the ball, like vpinball's environment reflection.
//   - The nearby table, sampled from the playfield art around the ball in the
//     direction each surface point faces. This moves with the ball, so it
//     reflects whatever it rolls over.
// Plus specular hotspots for the two overhead lights and a Fresnel rim.
//
// Crucially the vertex stage drops the ball's spin (uses only its world
// position), so the reflection stays put while the ball rotates - otherwise the
// reflection rides along and the ball reads as a flat spinning textured disc.

#import bevy_sprite::{
    mesh2d_functions as mesh_functions,
    mesh2d_vertex_output::VertexOutput,
}

const PI: f32 = 3.14159265359;

struct BallUniform {
    // rgb = reflection tint, a = specular shininess
    tint: vec4<f32>,
    // xy = screen-space direction to the light (y up), z = elevation, w = intensity
    light0: vec4<f32>,
    light1: vec4<f32>,
    // xy = playfield size (m), z = table reflection strength, w = reflection spread (m)
    playfield: vec4<f32>,
    // x = decal strength, y = decal mode (0 = scratches/additive, 1 = logo/screen)
    decal: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> ball: BallUniform;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var env_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var env_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var playfield_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var playfield_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var decal_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var decal_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let model = mesh_functions::get_world_from_local(vertex.instance_index);
    // Only the ball's world position (translation), not its rotation: a chrome
    // sphere's reflection is fixed in space, so the disc must stay screen-aligned
    // while the ball spins.
    let center = (model * vec4<f32>(0.0, 0.0, 0.0, 1.0)).xyz;
    let world_position = vec4<f32>(center + vertex.position, 1.0);
    out.world_position = world_position;
    out.position = mesh_functions::mesh2d_position_world_to_clip(world_position);
    out.uv = vertex.uv;
    // Pack the ball's z-rotation (cos, sin) so the fragment can spin the surface
    // decal with the ball. The reflection ignores this; scratches ride the surface.
    let x_basis = model[0].xyz;
    let inv = inverseSqrt(max(dot(x_basis.xy, x_basis.xy), 1e-8));
    out.world_normal = vec3<f32>(x_basis.x * inv, x_basis.y * inv, 0.0);
    return out;
}

// Equirectangular lookup for a reflected direction. "Up" is the view axis (+z) in
// this top-down view: the ball's centre faces the ceiling and its rim faces the
// table, so latitude is measured from +z. That puts the environment's top
// (ceiling) in the centre of the ball rather than along its back edge.
fn sample_env(dir: vec3<f32>) -> vec3<f32> {
    let u = atan2(dir.x, dir.y) / (2.0 * PI) + 0.5;
    let v = 0.5 - asin(clamp(dir.z, -1.0, 1.0)) / PI;
    return textureSample(env_texture, env_sampler, vec2<f32>(u, v)).rgb;
}

fn specular(n: vec3<f32>, v: vec3<f32>, light: vec4<f32>, shininess: f32) -> f32 {
    let l = normalize(vec3<f32>(light.xy, light.z));
    let h = normalize(l + v);
    return pow(max(dot(n, h), 0.0), shininess) * light.w;
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    // Reconstruct the sphere normal across the disc (screen space, y up). uv is
    // screen-aligned because the vertex stage dropped the ball's spin.
    let nx = mesh.uv.x * 2.0 - 1.0;
    let ny = 1.0 - mesh.uv.y * 2.0;
    let r2 = nx * nx + ny * ny;
    let nz = sqrt(max(1.0 - r2, 0.0));
    let n = vec3<f32>(nx, ny, nz);

    let v = vec3<f32>(0.0, 0.0, 1.0); // orthographic top-down view direction
    let reflected = reflect(-v, n);

    // Distant environment reflection (does not move with the ball).
    let env = sample_env(reflected);

    // Nearby table reflection: sample the playfield art around the ball in the
    // direction each point faces. World position -> playfield UV (the playfield is
    // a table-sized quad centred on the origin; v is flipped since y is up).
    let size = ball.playfield.xy;
    let sample_world = mesh.world_position.xy + n.xy * ball.playfield.w;
    let pf_uv = vec2<f32>(
        sample_world.x / size.x + 0.5,
        0.5 - sample_world.y / size.y,
    );
    let pf = textureSample(playfield_texture, playfield_sampler, pf_uv).rgb;

    // The rim faces outward and reflects the table; the centre faces the camera
    // and reflects the environment.
    let rim = pow(1.0 - nz, 2.0);
    let reflection = mix(env, pf, rim * ball.playfield.z);

    var color = reflection * ball.tint.rgb;

    // Surface decal (wear/scratches or a logo). Unlike the reflection it lives on
    // the ball, so spin it with the ball using the packed rotation. The disc
    // coords map straight onto the front-facing hemisphere (orthographic).
    let ca = mesh.world_normal.x;
    let sa = mesh.world_normal.y;
    let surface = vec2<f32>(nx * ca + ny * sa, -nx * sa + ny * ca);
    let decal = textureSample(decal_texture, decal_sampler, surface * 0.5 + 0.5);
    let decal_amount = decal.rgb * decal.a * ball.decal.x;
    if (ball.decal.y > 0.5) {
        // Logo: screen blend over the reflection.
        color = 1.0 - (1.0 - color) * (1.0 - decal_amount);
    } else {
        // Scratches: additive, light catching the surface wear.
        color += decal_amount;
    }

    // Crisp hotspots for the two overhead lights.
    let shininess = ball.tint.a;
    let spec = specular(n, v, ball.light0, shininess) + specular(n, v, ball.light1, shininess);
    color += vec3<f32>(spec);

    // Fresnel: chrome reflects brighter at grazing angles -> bright rim.
    let fresnel = pow(1.0 - nz, 4.0);
    color += vec3<f32>(fresnel) * 0.2;

    // Antialiased circular edge.
    let r = sqrt(r2);
    let aa = fwidth(r) + 1e-4;
    let alpha = 1.0 - smoothstep(1.0 - aa, 1.0, r);

    return vec4<f32>(color, alpha);
}

// Chrome wire ramp. A wire ramp is drawn as a thin ribbon following the wire path; we
// shade it like a polished steel cylinder so it reflects the environment the way the
// chrome ball does (shaders/ball.wgsl), just with a cylinder normal instead of a sphere.
//
// Across the ribbon the surface normal bows like a half-tube: facing the viewer (+z) down
// the centerline and tilting out to the wire's in-plane cross direction at each edge.
// Along the wire the normal is constant - that is what makes it a cylinder, not a sphere.
// The mesh supplies the cross direction (vertex normal, in-plane) and the across-ribbon
// coordinate (uv.x, 0..1); see vpx::ramp_mesh::append_wire_ribbon.

#import bevy_sprite::{
    mesh2d_functions as mesh_functions,
    mesh2d_vertex_output::VertexOutput,
}

const PI: f32 = 3.14159265359;

struct WireUniform {
    // rgb = reflection tint, a = specular shininess
    tint: vec4<f32>,
    // xy = screen-space direction to the light (y up), z = elevation, w = intensity
    light0: vec4<f32>,
    light1: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> wire: WireUniform;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var env_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var env_sampler: sampler;

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
    let world_position = model * vec4<f32>(vertex.position, 1.0);
    out.world_position = world_position;
    out.position = mesh_functions::mesh2d_position_world_to_clip(world_position);
    out.uv = vertex.uv;
    // Ramp transforms are translation-only, so the in-plane cross direction passes
    // straight through to the fragment stage.
    out.world_normal = vertex.normal;
    return out;
}

// Equirectangular lookup for a reflected direction, matching the ball: "up" is the view
// axis (+z) in this top-down view, so latitude is measured from +z.
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
    // Cylinder normal: bow around the ribbon's cross direction. across in 0..1 maps to an
    // angle from +cross (one edge) through +z (centre) to -cross (other edge).
    let cross = normalize(mesh.world_normal.xy);
    let across = mesh.uv.x;
    let theta = (0.5 - across) * PI;
    let s = sin(theta);
    let c = cos(theta);
    let n = vec3<f32>(cross * s, c);

    let v = vec3<f32>(0.0, 0.0, 1.0); // orthographic top-down view direction
    let reflected = reflect(-v, n);

    let env = sample_env(reflected);
    var color = env * wire.tint.rgb;

    // Crisp hotspots for the two overhead lights.
    let shininess = wire.tint.a;
    let spec = specular(n, v, wire.light0, shininess) + specular(n, v, wire.light1, shininess);
    color += vec3<f32>(spec);

    // Fresnel: chrome reflects brighter at grazing angles, brightening the tube edges.
    let fresnel = pow(1.0 - max(c, 0.0), 4.0);
    color += vec3<f32>(fresnel) * 0.2;

    return vec4<f32>(color, 1.0);
}

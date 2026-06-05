// Composites the offscreen light/shadow map onto the playfield. The light map is
// rendered by a dedicated camera that sees only the light/shadow layer over the
// exact playfield rect, so it shares the playfield's UVs. Multiplying modulates
// the playfield by the lighting: bright where lit, dark where shadowed, and the
// map is clipped to the playfield by construction (no matte needed).

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var playfield_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var playfield_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var light_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var light_sampler: sampler;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let base = textureSample(playfield_texture, playfield_sampler, mesh.uv);
    let light = textureSample(light_texture, light_sampler, mesh.uv);
    return vec4<f32>(base.rgb * light.rgb, 1.0);
}

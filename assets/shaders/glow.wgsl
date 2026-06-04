// Additive glow material for table lights. The pipeline blend state is set to
// additive in `GlowMaterial::specialize`, so this shader outputs a premultiplied
// colour: the radial texture's alpha weights the light colour, and transparent
// edges add nothing to the framebuffer. Light adds to the playfield instead of
// replacing it, so colours are brightened rather than washed out.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> color: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var glow_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var glow_sampler: sampler;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let sample = textureSample(glow_texture, glow_sampler, mesh.uv);
    let intensity = sample.a * color.a;
    return vec4<f32>(color.rgb * intensity, intensity);
}

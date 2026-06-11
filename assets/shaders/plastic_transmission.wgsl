// Translucent plastic over the playfield: the usual tinted texture, plus the light
// below transmitted through and tinted by the plastic (vpinball's bulb transmission,
// BasicShader "add light from below"). The wall/ramp meshes carry table-space UVs,
// which are exactly the light map's UVs, so the plastic samples the light shining
// underneath itself. Blending is premultiplied (see the material's specialize) so
// the transmitted light adds on top of the alpha-blended base colour.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct PlasticMaterial {
    // Tint and opacity of the plastic (the vpx material base colour + opacity).
    color: vec4<f32>,
    // How much of the light below is transmitted through the plastic.
    transmission: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: PlasticMaterial;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var light_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var light_sampler: sampler;

// Scale on the sampled light map; the map's ambient clear is pre-divided by this.
// Keep in sync with LIGHT_OVERBRIGHT in lightmap.rs (and playfield_light.wgsl).
const OVERBRIGHT: f32 = 1.5;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let tex = textureSample(base_texture, base_sampler, mesh.uv);
    let light = textureSample(light_texture, light_sampler, mesh.uv) * OVERBRIGHT;
    let base = material.color * tex;
    // Transmitted light: tinted by the plastic colour (sqrt reads brighter, like
    // vpinball), scaled by the light under this spot. Masked so cut-out holes
    // (texture alpha 0) transmit nothing and near-opaque print blocks the light.
    let coverage = smoothstep(0.0, 0.05, tex.a) * (1.0 - base.a);
    let transmitted = sqrt(material.color.rgb) * light.rgb * material.transmission * coverage;
    // Premultiplied output: base is weighted by its own alpha, transmission adds.
    return vec4<f32>(base.rgb * base.a + transmitted, base.a);
}

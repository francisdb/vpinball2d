// Animated insert light, vpinball's classic light composite (ClassicLightShader's
// PS_LightWithTexel): the light colour times the radial falloff is added over the
// insert's art, then the art is re-composited with Overlay (darks like decal
// prints stay dark) and Screen (the art brightens the result). The output is
// premultiplied by the saturating falloff-times-intensity, crossfading from the
// unlit framebuffer to the fully lit insert as the animation raises the intensity.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct InsertLight {
    // rgb: the light colour at the falloff edge (linear); a: the current animated
    // intensity in raw vpx units (inserts author 10-90; the saturate below does
    // the rest).
    color: vec4<f32>,
    // rgb: the light colour at the centre (vpx "color full").
    color_full: vec4<f32>,
    // The vpx falloff power shaping the attenuation curve (default 2).
    falloff_power: f32,
    // Playfield extent in world metres (the table is centred on the origin).
    table_size: vec2<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: InsertLight;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var art_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var art_sampler: sampler;

// vpinball's OverlayHDR (Helpers.fxh): darks darken (2ab), brights screen.
fn overlay(base: vec3<f32>, blend: vec3<f32>) -> vec3<f32> {
    let pick = step(vec3<f32>(0.5), base);
    return max(
        mix(base * blend * 2.0, 1.0 - 2.0 * (1.0 - base) * (1.0 - blend), pick),
        vec3<f32>(0.0),
    );
}

// vpinball's ScreenHDR (Helpers.fxh).
fn screen(base: vec3<f32>, blend: vec3<f32>) -> vec3<f32> {
    return max(1.0 - (1.0 - base) * (1.0 - blend), vec3<f32>(0.0));
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    // The mesh UVs map the falloff radius to 0.5 from the centre (insert shapes)
    // or the halo rim to the rect edge (bulb discs), so this is distance over the
    // falloff range - vpinball's `len`.
    let len = length(mesh.uv - vec2<f32>(0.5)) * 2.0;
    let atten = pow(clamp(1.0 - len, 0.0, 1.0), material.falloff_power);
    let strength = clamp(atten * material.color.a, 0.0, 1.0);
    // The art under this fragment, sampled in table space like vpinball's auto
    // texture coordinates.
    let art_uv = vec2<f32>(
        mesh.world_position.x / material.table_size.x + 0.5,
        0.5 - mesh.world_position.y / material.table_size.y,
    );
    let art = textureSample(art_texture, art_sampler, art_uv).rgb;
    // vpinball lerps the light colour from "color full" at the centre to the
    // edge colour over sqrt of the falloff distance (both light shaders).
    let lcolor = mix(material.color_full.rgb, material.color.rgb, sqrt(min(len, 1.0)));
    var lit = art + lcolor * (atten * material.color.a);
    // vpinball runs this composite in HDR and tonemaps; in LDR, clamping before
    // the overlay is what keeps dark art (decal prints) readable on a lit insert
    // instead of washing it out with the unbounded added intensity.
    lit = clamp(lit, vec3<f32>(0.0), vec3<f32>(1.0));
    lit = overlay(art, lit);
    lit = screen(art, lit);
    return vec4<f32>(lit * strength, strength);
}

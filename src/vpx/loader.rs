use crate::vpx::VpxAsset;
use crate::vpx::primitive_mesh;
use crate::vpx::ramp_mesh;
use crate::vpx::triangulate::triangulate_polygon;
use bevy::asset::{LoadDirectError, RenderAssetUsages};
use bevy::image::{CompressedImageFormats, ImageLoader, ImageLoaderError};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::{
    asset::{AssetLoader, LoadContext, io::Reader},
    prelude::*,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;
use vpin::vpx::gameitem::GameItemEnum;
use vpin::vpx::gameitem::dragpoint::DragPoint;
use vpin::vpx::image::ImageData;
use vpin::vpx::material::{Material, MaterialType, SaveMaterial};
use vpin::vpx::sound::write_sound;
use vpin::vpx::units::vpu_to_m;

/// An error that occurs when loading a vpx file.
#[derive(Error, Debug)]
pub enum VpxError {
    /// An [IO](std::io) Error
    #[error("Could not load asset: {0}")]
    Io(#[from] std::io::Error),
    /// A LoadDirectError Error
    #[error("Could not load: {0}")]
    BevyLoadDirectError(#[from] LoadDirectError),
    /// A ImageLoaderError Error
    #[error("Could not load image: {0}")]
    ImageLoaderError(#[from] ImageLoaderError),
}

#[derive(Serialize, Deserialize)]
pub struct VpxLoaderSettings {
    pub load_images: bool,
    pub load_sounds: bool,
    pub load_meshes: bool,
}

impl Default for VpxLoaderSettings {
    fn default() -> Self {
        Self {
            load_images: true,
            load_sounds: true,
            load_meshes: true,
        }
    }
}

/// Loads vpx files with all of their data as their corresponding bevy representations.
#[derive(TypePath)]
pub struct VpxLoader {}

impl AssetLoader for VpxLoader {
    type Asset = VpxAsset;
    type Settings = VpxLoaderSettings;
    type Error = VpxError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &VpxLoaderSettings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        info!("Loading VPX {}", load_context.path());
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Self::load_vpx(self, &bytes, load_context, settings).await
    }

    fn extensions(&self) -> &[&str] {
        &["vpx"]
    }
}

impl VpxLoader {
    async fn load_vpx(
        &self,
        bytes: &[u8],
        load_context: &mut LoadContext<'_>,
        settings: &VpxLoaderSettings,
    ) -> Result<VpxAsset, VpxError> {
        let mut vpx = vpin::vpx::from_bytes(bytes).map_err(|e| {
            VpxError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to parse VPX file: {e}"),
            ))
        })?;

        // Tables saved before VPX 10.8 store materials only in the legacy MATE format.
        // Convert them into the regular list so the rest of the code has a single
        // material source, like vpinball does on load.
        if vpx.gamedata.materials.is_none() {
            vpx.gamedata.materials = Some(
                vpx.gamedata
                    .materials_old
                    .iter()
                    .map(material_from_save_material)
                    .collect(),
            );
        }

        let mut image_handles = Vec::new();
        let mut named_image_handles = HashMap::new();
        if settings.load_images {
            for image in &mut vpx.images {
                let label = format!("images/{}", image.name);
                // VPX stores images either as a compressed blob (`jpeg`, despite the
                // name this can be JPEG/PNG/etc.) or as a raw bitmap (`bits`, always
                // BGRA). Load whichever is present.
                let handle = if let Some(jpeg) = &image.jpeg {
                    match load_image(label, load_context, image, jpeg.data.clone()).await {
                        Ok((handle, computed_opaque)) => {
                            // Old tables do not store the opaque flag; fill it in from the
                            // decoded pixels like vpinball (BaseTexture::IsOpaque) so the
                            // renderers can rely on it for alpha handling.
                            if image.is_opaque.is_none() {
                                image.is_opaque = computed_opaque;
                            }
                            Some(handle)
                        }
                        Err(e) => {
                            // TODO we could retry loading the image and let the image loader guess the
                            //   format; sometimes vpx files have images with the wrong extension.
                            error!("Failed to load image {}: {}", image.name, e);
                            None
                        }
                    }
                } else if let Some(bits) = &image.bits {
                    let (handle, computed_opaque) = load_bitmap(label, load_context, image, bits);
                    if image.is_opaque.is_none() {
                        image.is_opaque = computed_opaque;
                    }
                    handle
                } else {
                    warn!("Image: {} Path: {} No image data", image.name, image.path);
                    None
                };
                let Some(handle) = handle else {
                    continue;
                };
                if !image.name.is_empty() {
                    // VPinball matches image names case-insensitively; store a
                    // lowercased key and look up the same way (see VpxAsset::image).
                    named_image_handles
                        .insert(image.name.to_lowercase().into_boxed_str(), handle.clone());
                }
                image_handles.push(handle);
            }
        }

        let mut sound_handles = Vec::new();
        let mut named_sound_handles = HashMap::new();
        if settings.load_sounds {
            for sound in &vpx.sounds {
                let handle =
                    load_sound(format!("sounds/{}", sound.name), load_context, sound).await;
                match handle {
                    Ok(handle) => {
                        if !sound.name.is_empty() {
                            named_sound_handles
                                .insert(sound.name.clone().into_boxed_str(), handle.clone());
                        }
                        sound_handles.push(handle);
                    }
                    Err(e) => {
                        error!("Failed to load sound {}: {}", sound.name, e);
                        continue;
                    }
                }
            }
        }

        let mut mesh_handles = Vec::new();
        let mut named_mesh_handles = HashMap::new();
        let mut named_mesh_centers = HashMap::new();
        // Table size in vpx units; wall/ramp world-aligned UVs are normalized by it.
        let table_size = Vec2::new(
            vpx.gamedata.right - vpx.gamedata.left,
            vpx.gamedata.bottom - vpx.gamedata.top,
        );
        if settings.load_meshes {
            for item in &vpx.gameitems {
                // Walls get a generated 2D mesh; rubbers build their own ring mesh at spawn
                // time (see pinball::rubber) and other items have no generated mesh.
                if let GameItemEnum::Wall(wall) = item {
                    let path = VpxAsset::wall_mesh_sub_path(&wall.name);
                    let handle = load_mesh_2d_from_drag_points(
                        table_size,
                        path.clone(),
                        &wall.drag_points,
                        load_context,
                    );
                    named_mesh_handles.insert(path.into_boxed_str(), handle.clone());
                    mesh_handles.push(handle);
                } else if let GameItemEnum::Ramp(ramp) = item {
                    // Ramps are open paths (not looped); build their top-down silhouette.
                    let mut centerline: Vec<Vec2> =
                        vpin::vpx::mesh::smooth_drag_points_2d(&ramp.drag_points, 4.0, false)
                            .into_iter()
                            .map(|(x, y)| Vec2::new(x, y))
                            .collect();
                    // vpin's open-curve smoothing drops the final drag point (vpinball
                    // appends it explicitly, see dragpoint.h "Add the very last point").
                    // Without it a straight 2-point ramp (e.g. an apron score card)
                    // collapses to a single point and gets no mesh.
                    // TODO fix upstream in vpin and drop this.
                    if let Some(last) = ramp.drag_points.last() {
                        let end = Vec2::new(last.x, last.y);
                        if centerline.last() != Some(&end) {
                            centerline.push(end);
                        }
                    }
                    if let Some(mesh) = ramp_mesh::build_ramp_mesh_2d(table_size, ramp, centerline)
                    {
                        let path = VpxAsset::ramp_mesh_sub_path(&ramp.name);
                        let labeled = load_context.begin_labeled_asset();
                        let handle = load_context
                            .add_loaded_labeled_asset(path.clone(), labeled.finish(mesh));
                        named_mesh_handles.insert(path.into_boxed_str(), handle.clone());
                        mesh_handles.push(handle);
                    }
                } else if let GameItemEnum::Primitive(primitive) = item {
                    // Visible primitives are projected to their top-down silhouette (only the
                    // upward-facing faces). Invisible primitives are skipped.
                    if primitive.is_visible
                        && let Some((mesh, center_z)) =
                            primitive_mesh::build_primitive_mesh_2d(primitive)
                    {
                        let path = VpxAsset::primitive_mesh_sub_path(&primitive.name);
                        let labeled = load_context.begin_labeled_asset();
                        let handle = load_context
                            .add_loaded_labeled_asset(path.clone(), labeled.finish(mesh));
                        named_mesh_centers.insert(path.clone().into_boxed_str(), center_z);
                        named_mesh_handles.insert(path.into_boxed_str(), handle.clone());
                        mesh_handles.push(handle);
                    }
                }
            }
        }

        let custom_asset = VpxAsset {
            images: image_handles,
            named_images: named_image_handles,
            sounds: sound_handles,
            named_sounds: named_sound_handles,
            meshes: mesh_handles,
            named_meshes: named_mesh_handles,
            named_mesh_centers,
            raw: vpx,
        };

        Ok(custom_asset)
    }
}

/// Convert a legacy (pre 10.8) [`SaveMaterial`] to a [`Material`], inverting the
/// quantization vpinball applies when saving (see `From<&Material> for SaveMaterial`
/// in vpin and `Material::Material(const SaveMaterial&)` in vpinball). The physics
/// fields are private on [`Material`] and stay at their defaults; physics comes from
/// the game items themselves.
fn material_from_save_material(save: &SaveMaterial) -> Material {
    // Material has private fields, so a struct literal is not possible; start from
    // the default and overwrite the visual fields.
    let mut material = Material::default();
    material.name = save.name.clone();
    material.type_ = if save.is_metal {
        MaterialType::Metal
    } else {
        MaterialType::Basic
    };
    material.wrap_lighting = save.wrap_lighting;
    material.roughness = save.roughness;
    // Saved as `255 - quantize_u8(8, lerp)` for compatibility with old table versions.
    material.glossy_image_lerp = f32::from(255 - save.glossy_image_lerp) / 255.0;
    material.thickness = f32::from(save.thickness) / 255.0;
    material.edge = save.edge;
    material.opacity = save.opacity;
    material.base_color = save.base_color;
    material.glossy_color = save.glossy_color;
    material.clearcoat_color = save.clearcoat_color;
    // Bit 0 is the opacity-active flag, the upper 7 bits hold the quantized edge alpha.
    material.opacity_active = save.opacity_active_edge_alpha & 1 != 0;
    material.edge_alpha = f32::from(save.opacity_active_edge_alpha >> 1) / 127.0;
    material
}

/// Whether every pixel of the image is fully opaque, like vpinball's
/// `BaseTexture::IsOpaque`. `None` when the format has no simple 8-bit alpha
/// channel to inspect (treated as opaque downstream).
fn compute_is_opaque(image: &Image) -> Option<bool> {
    use bevy::render::render_resource::TextureFormat;
    match image.texture_descriptor.format {
        TextureFormat::Rgba8Unorm
        | TextureFormat::Rgba8UnormSrgb
        | TextureFormat::Bgra8Unorm
        | TextureFormat::Bgra8UnormSrgb => image
            .data
            .as_ref()
            .map(|data| data.chunks_exact(4).all(|pixel| pixel[3] == 255)),
        _ => None,
    }
}

async fn load_image(
    label: String,
    load_context: &mut LoadContext<'_>,
    image_data: &ImageData,
    bytes: Vec<u8>,
) -> Result<(Handle<Image>, Option<bool>), <VpxLoader as AssetLoader>::Error> {
    let mut reader = bevy::asset::io::VecReader::new(bytes);
    // TODO how do we properly delegate here to an Image AssetLoader?
    // // use the load context to load the image data from bytes
    // let image_asset = load_context
    //     .loader()
    //     .immediate()
    //     .with_reader(&mut reader)
    //     .with_unknown_type()
    //     .load(ball_image.path)
    //     .await?
    //     .downcast::<Image>().ok().unwrap();

    // TODO how do we get an image loader instead of creating a new one here?
    let image_loader = ImageLoader::new(CompressedImageFormats::all());
    let path = Path::new(&image_data.path);
    let image_format = ImageFormat::from_extension(path.extension().unwrap().to_str().unwrap());
    let format_setting = match image_format {
        Some(fmt) => bevy::image::ImageFormatSetting::Format(fmt),
        None => bevy::image::ImageFormatSetting::Guess,
    };
    let settings = bevy::image::ImageLoaderSettings {
        format: format_setting,
        ..default()
    };
    let mut labeled = load_context.begin_labeled_asset();
    let image = image_loader
        .load(&mut reader, &settings, &mut labeled)
        .await?;
    let is_opaque = compute_is_opaque(&image);
    let loaded = labeled.finish(image);
    let handle = load_context.add_loaded_labeled_asset(label, loaded);
    Ok((handle, is_opaque))
}

/// Build a Bevy image from a VPX raw bitmap (`bits`): LZW-compressed BGRA pixels.
/// Returns `None` (best effort: skip just this image) if the decoded data does
/// not match the declared dimensions.
fn load_bitmap(
    label: String,
    load_context: &mut LoadContext<'_>,
    image_data: &ImageData,
    bits: &vpin::vpx::image::ImageDataBits,
) -> (Option<Handle<Image>>, Option<bool>) {
    let bgra = vpin::vpx::lzw::from_lzw_blocks(&bits.lzw_compressed_data);
    let expected = image_data.width as usize * image_data.height as usize * 4;
    if bgra.len() != expected {
        error!(
            "Bitmap '{}' is {}x{} but decoded to {} bytes (expected {expected})",
            image_data.name,
            image_data.width,
            image_data.height,
            bgra.len(),
        );
        return (None, None);
    }
    let is_opaque = bgra.chunks_exact(4).all(|pixel| pixel[3] == 255);
    let image = Image::new(
        Extent3d {
            width: image_data.width,
            height: image_data.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        bgra,
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::default(),
    );
    (
        Some(load_context.add_labeled_asset(label, image)),
        Some(is_opaque),
    )
}

async fn load_sound(
    label: String,
    load_context: &mut LoadContext<'_>,
    sound: &vpin::vpx::sound::SoundData,
) -> Result<Handle<AudioSource>, <VpxLoader as AssetLoader>::Error> {
    let bytes = write_sound(sound);
    let mut reader = bevy::asset::io::VecReader::new(bytes);
    let audio_loader = bevy::audio::AudioLoader;
    let settings = ();
    let mut labeled = load_context.begin_labeled_asset();
    let audio_source = audio_loader
        .load(&mut reader, &settings, &mut labeled)
        .await?;
    let handle = load_context.add_loaded_labeled_asset(label, audio_source.into());
    Ok(handle)
}

/// Generates a flat 2D polygon mesh from the given drag points. The mesh lies at z 0;
/// the spawner puts the wall's top height into the entity transform so transparent
/// 2D sorting (which only sees the transform) layers it correctly, e.g. an apron
/// drawn over the ball rolling underneath it.
fn load_mesh_2d_from_drag_points(
    table_size: Vec2,
    label: String,
    drag_points: &[DragPoint],
    load_context: &mut LoadContext<'_>,
) -> Handle<Mesh> {
    // Round the outline like Visual Pinball: smooth the drag points with the same
    // Catmull-Rom spline VPX uses for wall/rubber meshes (closed loop, max accuracy 4.0).
    // The smoothed points are in vpx units, like the raw drag-point coordinates.
    let smoothed = vpin::vpx::mesh::smooth_drag_points_2d(drag_points, 4.0, true);
    let num_points = smoothed.len();
    let mut positions = Vec::with_capacity(num_points);
    let mut uvs = Vec::with_capacity(num_points);

    for (x, y) in &smoothed {
        positions.push([vpu_to_m(*x), -vpu_to_m(*y), 0.0]);
        // Wall top textures use table-space UVs (auto texture coordinates),
        // normalized so the image spans the whole table.
        uvs.push([x / table_size.x, y / table_size.y]);
    }

    // Triangulate the polygon using ear clipping (works for any polygon)
    // points should be counter-clockwise but this is already ensured by vpx
    let positions_2d: Vec<Vec2> = positions
        .iter()
        .map(|p| Vec2::new(p[0], p[1])) // Use x,y as 2D coordinates
        .collect();

    let indices = triangulate_polygon(&positions_2d);

    // let mesh = Mesh::from(Polyline2d::new(vertices));
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    // mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    let labeled = load_context.begin_labeled_asset();
    load_context.add_loaded_labeled_asset(label, labeled.finish(mesh))
}

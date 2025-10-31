use crate::vpx::VpxAsset;
use crate::vpx::triangulate::triangulate_polygon;
use bevy::asset::{LoadDirectError, RenderAssetUsages};
use bevy::image::{CompressedImageFormats, ImageLoader, ImageLoaderError};
use bevy::mesh::{Indices, PrimitiveTopology};
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
use vpin::vpx::sound::write_sound;
use vpin::vpx::vpu_to_m;

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
        info!("Loading VPX {}", load_context.path().display());
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
        let vpx = vpin::vpx::from_bytes(bytes).map_err(|e| {
            VpxError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to parse VPX file: {e}"),
            ))
        })?;

        let mut image_handles = Vec::new();
        let mut named_image_handles = HashMap::new();
        if settings.load_images {
            for image in &vpx.images {
                if let Some(jpeg) = &image.jpeg {
                    let bytes = jpeg.data.clone();
                    let handle =
                        load_image(format!("images/{}", image.name), load_context, image, bytes)
                            .await;
                    match handle {
                        Ok(handle) => {
                            if !image.name.is_empty() {
                                named_image_handles
                                    .insert(image.name.clone().into_boxed_str(), handle.clone());
                            }
                            image_handles.push(handle);
                        }
                        Err(e) => {
                            // TODO we could retry loading the image and let the image loader guess the format
                            //   sometimes vpx files have images with the wrong extension.
                            error!("Failed to load image {}: {}", image.name, e);
                            continue;
                        }
                    }
                } else {
                    warn!("Image: {} Path: {} No JPEG data", image.name, image.path);
                }
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
        // TODO where does the 100.0 factor come from?
        let table_size = Vec2::new(
            (vpx.gamedata.right - vpx.gamedata.left) / 100.0,
            (vpx.gamedata.bottom - vpx.gamedata.top) / 100.0,
        );
        if settings.load_meshes {
            for item in &vpx.gameitems {
                match item {
                    GameItemEnum::Wall(wall) => {
                        let top_height = vpu_to_m(wall.height_top);
                        let path = VpxAsset::wall_mesh_sub_path(&wall.name);
                        let handle = load_mesh_2d_from_drag_points(
                            table_size,
                            path.clone(),
                            &wall.drag_points,
                            top_height,
                            load_context,
                        );
                        named_mesh_handles.insert(path.into_boxed_str(), handle.clone());
                        mesh_handles.push(handle);
                    }
                    GameItemEnum::Rubber(rubber) => {
                        // a rubber is presented by a ring shape formed by the rubber.drag_points
                        // with the thickness rubber.thickness
                        let top_height = vpu_to_m(rubber.height + rubber.thickness as f32 / 2.0);
                        let path = VpxAsset::rubber_mesh_sub_path(&rubber.name);
                        let handle = load_mesh_2d_from_drag_points(
                            table_size,
                            path.clone(),
                            &rubber.drag_points,
                            top_height,
                            load_context,
                        );
                        named_mesh_handles.insert(path.into_boxed_str(), handle.clone());
                        mesh_handles.push(handle);
                    }
                    _ => {}
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
            raw: vpx,
        };

        Ok(custom_asset)
    }
}

async fn load_image(
    label: String,
    load_context: &mut LoadContext<'_>,
    image_data: &ImageData,
    bytes: Vec<u8>,
) -> Result<Handle<Image>, <VpxLoader as AssetLoader>::Error> {
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
    let loaded = labeled.finish(image);
    let handle = load_context.add_loaded_labeled_asset(label, loaded);
    Ok(handle)
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

/// Generates a flat 2D polygon mesh from the given drag points at the specified top height.
fn load_mesh_2d_from_drag_points(
    table_size: Vec2,
    label: String,
    drag_points: &Vec<DragPoint>,
    top_height: f32,
    load_context: &mut LoadContext<'_>,
) -> Handle<Mesh> {
    // Generate vertices for top face (all with the same height)
    let num_points = drag_points.len();
    let mut positions = Vec::with_capacity(num_points);
    let mut normals = Vec::with_capacity(num_points);
    let mut uvs = Vec::with_capacity(num_points);

    for point in drag_points {
        // Position (x, top_height, y) -> Bevy uses y-up
        positions.push([vpu_to_m(point.x), -vpu_to_m(point.y), top_height]);
        // Normal points up for the top face
        normals.push([0.0, 0.0, 1.0]);
        if point.has_auto_texture {
            uvs.push([point.x / table_size.x, point.y / table_size.y]);
        } else {
            warn!(
                "Handle non-auto texture coordinates for mesh generation, should we use tex_coord?"
            );
            uvs.push([point.x, point.y]);
        }
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

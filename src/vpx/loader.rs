use crate::vpx::VpxAsset;
use bevy::asset::LoadDirectError;
use bevy::image::{CompressedImageFormats, ImageLoader, ImageLoaderError};
use bevy::{
    asset::{AssetLoader, LoadContext, io::Reader},
    prelude::*,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;
use vpin::vpx::image::ImageData;
use vpin::vpx::sound::write_sound;

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
}

impl Default for VpxLoaderSettings {
    fn default() -> Self {
        Self {
            load_images: true,
            load_sounds: true,
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
                format!("Failed to parse VPX file: {}", e),
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
                            .await?;
                    if !image.name.is_empty() {
                        named_image_handles
                            .insert(image.name.clone().into_boxed_str(), handle.clone());
                    }
                    image_handles.push(handle);
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
                    load_sound(format!("sounds/{}", sound.name), load_context, sound).await?;
                if !sound.name.is_empty() {
                    named_sound_handles.insert(sound.name.clone().into_boxed_str(), handle.clone());
                }
                sound_handles.push(handle);
            }
        }

        let custom_asset = VpxAsset {
            images: image_handles,
            named_images: named_image_handles,
            sounds: sound_handles,
            named_sounds: named_sound_handles,
            raw: vpx,
        };

        Ok(custom_asset)
    }
}

async fn load_image(
    label: String,
    load_context: &mut LoadContext<'_>,
    ball_image: &ImageData,
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
    let path = Path::new(&ball_image.path);
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

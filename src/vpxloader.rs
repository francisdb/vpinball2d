use bevy::asset::LoadDirectError;
use bevy::image::{CompressedImageFormats, ImageLoader, ImageLoaderError};
use bevy::{
    asset::{AssetLoader, LoadContext, io::Reader},
    prelude::*,
    reflect::TypePath,
};
use std::path::Path;
use thiserror::Error;
use vpin::vpx::image::ImageData;
use vpin::vpx::sound::write_sound;

pub(super) fn plugin(app: &mut App) {
    // vpx loading
    app.init_asset::<VpxAsset>()
        .init_asset_loader::<VpxAssetLoader>();
}

#[derive(Asset, TypePath, Debug /*Deserialize*/)]
pub(crate) struct VpxAsset {
    _gravity: f32,
    pub(crate) _ball_image: Handle<Image>,
    pub(crate) _playfield_image: Handle<Image>,
}

#[derive(Default)]
struct VpxAssetLoader;

/// Possible errors that can be produced by [`VpxAssetLoader`]
#[non_exhaustive]
#[derive(Debug, Error)]
enum CustomAssetLoaderError {
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

impl AssetLoader for VpxAssetLoader {
    type Asset = VpxAsset;
    type Settings = ();
    type Error = CustomAssetLoaderError;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        info!("Loading VPX file...");
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let vpx = vpin::vpx::from_bytes(&bytes).map_err(|e| {
            CustomAssetLoaderError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to parse VPX file: {}", e),
            ))
        })?;
        info!("Loaded VPX file named: {:?}", vpx.info.table_name);

        let ball_image = vpx
            .images
            .iter()
            .find(|img| img.name == vpx.gamedata.ball_image)
            .unwrap();
        // how do we avoid the clone here?
        let bytes = ball_image.jpeg.clone().unwrap().data;
        let ball_image =
            load_image("images/ballimage".into(), load_context, &ball_image, bytes).await?;

        let playfield_image = vpx
            .images
            .iter()
            .find(|img| img.name == vpx.gamedata.image)
            .unwrap();
        // how do we avoid the clone here?
        let bytes = playfield_image.jpeg.clone().unwrap().data;
        let playfield_image = load_image(
            "images/playfieldimage".into(),
            load_context,
            &playfield_image,
            bytes,
        )
        .await?;

        // We'll have to be a bit more creative here since ball sounds are actually handled by the script in vpinball.
        let rolling_sound = vpx
            .sounds
            .iter()
            .find(|sfx| sfx.name == "fx_ballrolling0")
            .unwrap();
        let rolling_sound_handle =
            load_sound("sounds/fx_ballrolling0".into(), load_context, rolling_sound).await?;

        let custom_asset = VpxAsset {
            _gravity: vpx.gamedata.gravity,
            _ball_image: ball_image,
            _playfield_image: playfield_image,
        };

        Ok(custom_asset)
    }

    fn extensions(&self) -> &[&str] {
        &["vpx"]
    }
}

async fn load_image(
    label: String,
    load_context: &mut LoadContext<'_>,
    ball_image: &&ImageData,
    bytes: Vec<u8>,
) -> Result<Handle<Image>, <VpxAssetLoader as AssetLoader>::Error> {
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
) -> Result<Handle<AudioSource>, <VpxAssetLoader as AssetLoader>::Error> {
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

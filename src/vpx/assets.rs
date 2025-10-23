//! Representation of assets present in a vpx file

use bevy::prelude::*;
use std::collections::HashMap;
use vpin::vpx::VPX;

/// Representation of a loaded vpx file.
#[derive(Asset, Debug, TypePath)]
pub struct VpxAsset {
    /// All images loaded from the vpx file.
    #[allow(dead_code)]
    pub images: Vec<Handle<Image>>,
    /// Named scenes loaded from the vpx file.
    pub named_images: HashMap<Box<str>, Handle<Image>>,
    /// All sounds loaded from the vpx file.
    #[allow(dead_code)]
    pub sounds: Vec<Handle<AudioSource>>,
    /// Named sounds loaded from the vpx file.
    pub named_sounds: HashMap<Box<str>, Handle<AudioSource>>,
    pub raw: VPX,
}

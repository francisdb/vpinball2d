//! Representation of assets present in a vpx file

use bevy::prelude::*;
use std::collections::HashMap;
use vpin::vpx::VPX;

/// Height range (vpx units) of a projected top-down mesh in world space.
#[derive(Debug, Clone, Copy)]
pub struct MeshHeights {
    /// Centre of the geometry's height range; the mesh vertices carry z offsets
    /// relative to it.
    pub center: f32,
    /// Lowest point of the geometry, e.g. where a post meets the playfield.
    pub base: f32,
}

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
    /// All meshes loaded from the vpx file.
    #[allow(dead_code)]
    pub meshes: Vec<Handle<Mesh>>,
    /// Named meshes loaded from the vpx file.
    pub named_meshes: HashMap<Box<str>, Handle<Mesh>>,
    /// Heights (vpx units) of each named mesh whose vertices are stored relative to
    /// the centre; the spawner puts the centre into the entity transform as the
    /// render layer and gates shadows on the base.
    pub named_mesh_heights: HashMap<Box<str>, MeshHeights>,
    /// The raw VPX data structure.
    pub raw: VPX,
}

impl VpxAsset {
    pub fn wall_mesh_sub_path(name: &str) -> String {
        format!("meshes/wall/{name}")
    }

    pub fn ramp_mesh_sub_path(name: &str) -> String {
        format!("meshes/ramp/{name}")
    }

    pub fn primitive_mesh_sub_path(name: &str) -> String {
        format!("meshes/primitive/{name}")
    }

    /// Look up a loaded image by name, case-insensitively (VPinball treats image
    /// names case-insensitively, and tables often differ only in case).
    pub fn image(&self, name: &str) -> Option<&Handle<Image>> {
        self.named_images.get(name.to_lowercase().as_str())
    }
}

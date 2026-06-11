//! 2D (top-down) rendering of vpinball primitives.
//!
//! A primitive is a baked 3D mesh placed by a transform. This project renders the table
//! top-down, so we only want the part visible from above: we transform the mesh to world
//! space, keep the triangles whose normal points up (the "top faces"), and project them
//! onto the playfield plane, keeping their UVs and using each vertex's world height as the
//! render z (depth ordering). Down-facing and edge-on faces are dropped.
//!
//! The world transform is ported exactly from vpinball / vpin's `primitive_world_matrix`
//! (`fullMat = Scale(size) * RT * Translate(pos)`, applied to row vectors), so primitives
//! land where vpinball puts them.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use vpin::vpx::gameitem::primitive::Primitive;
use vpin::vpx::units::vpu_to_m;

/// A row-major 4x4 matrix, matching vpinball's `Matrix3D` so vertices transform as the
/// row-vector product `v * M` (translation lives in row 3).
type Mat = [[f32; 4]; 4];

fn identity() -> Mat {
    let mut m = [[0.0; 4]; 4];
    for (i, row) in m.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    m
}

/// Row-major matrix product where `(a * b)` applied to a row vector means `a` first.
fn mul(a: Mat, b: Mat) -> Mat {
    let mut r = [[0.0; 4]; 4];
    for (i, row) in r.iter_mut().enumerate() {
        for (l, cell) in row.iter_mut().enumerate() {
            *cell = a[i][0] * b[0][l] + a[i][1] * b[1][l] + a[i][2] * b[2][l] + a[i][3] * b[3][l];
        }
    }
    r
}

fn translate(x: f32, y: f32, z: f32) -> Mat {
    let mut m = identity();
    m[3][0] = x;
    m[3][1] = y;
    m[3][2] = z;
    m
}

fn scale(x: f32, y: f32, z: f32) -> Mat {
    let mut m = identity();
    m[0][0] = x;
    m[1][1] = y;
    m[2][2] = z;
    m
}

fn rotate_x(rad: f32) -> Mat {
    let (s, c) = rad.sin_cos();
    let mut m = identity();
    m[1][1] = c;
    m[1][2] = s;
    m[2][1] = -s;
    m[2][2] = c;
    m
}

fn rotate_y(rad: f32) -> Mat {
    let (s, c) = rad.sin_cos();
    let mut m = identity();
    m[0][0] = c;
    m[0][2] = -s;
    m[2][0] = s;
    m[2][2] = c;
    m
}

fn rotate_z(rad: f32) -> Mat {
    let (s, c) = rad.sin_cos();
    let mut m = identity();
    m[0][0] = c;
    m[0][1] = s;
    m[1][0] = -s;
    m[1][1] = c;
    m
}

/// `Scale(size) * RT * Translate(pos)` (vpin's `primitive_world_matrix`). Position is
/// applied after scale + rotation so it is not itself scaled or rotated.
fn world_matrix(primitive: &Primitive) -> Mat {
    let pos = &primitive.position;
    let size = &primitive.size;
    let rot = &primitive.rot_and_tra;
    let rt = mul(
        mul(
            mul(
                mul(
                    mul(
                        mul(
                            translate(rot[3], rot[4], rot[5]),
                            rotate_z(rot[2].to_radians()),
                        ),
                        rotate_y(rot[1].to_radians()),
                    ),
                    rotate_x(rot[0].to_radians()),
                ),
                rotate_z(rot[8].to_radians()),
            ),
            rotate_y(rot[7].to_radians()),
        ),
        rotate_x(rot[6].to_radians()),
    );
    mul(
        mul(scale(size.x, size.y, size.z), rt),
        translate(pos.x, pos.y, pos.z),
    )
}

/// Transform a point (row vector `p * M`, affine, w = 1).
fn transform_point(m: &Mat, x: f32, y: f32, z: f32) -> [f32; 3] {
    [
        m[0][0] * x + m[1][0] * y + m[2][0] * z + m[3][0],
        m[0][1] * x + m[1][1] * y + m[2][1] * z + m[3][1],
        m[0][2] * x + m[1][2] * y + m[2][2] * z + m[3][2],
    ]
}

/// Transform a direction (linear part only). Good enough for the facing sign that the
/// top-face test needs; primitives here use near-uniform positive scale.
fn transform_dir_z(m: &Mat, x: f32, y: f32, z: f32) -> f32 {
    m[0][2] * x + m[1][2] * y + m[2][2] * z
}

/// Build the top-down mesh for a primitive, or `None` if it has no decodable mesh or no
/// upward-facing geometry. Also returns the heights (in vpx units) of the kept
/// geometry: the spawner puts the centre into the entity transform as the render
/// layer (see `pinball::layer`, the mesh vertices carry z offsets relative to it)
/// and gates shadow casting on the base.
pub fn build_primitive_mesh_2d(
    primitive: &Primitive,
) -> Option<(Mesh, crate::vpx::assets::MeshHeights)> {
    let read = primitive.read_mesh().ok().flatten()?;
    if read.vertices.is_empty() || read.indices.is_empty() {
        return None;
    }
    let m = world_matrix(primitive);

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut min_z = f32::MAX;
    let mut max_z = f32::MIN;

    for face in &read.indices {
        let tri = [face.i0 as usize, face.i1 as usize, face.i2 as usize];
        if tri.iter().any(|&i| i >= read.vertices.len()) {
            continue;
        }
        // Keep only top faces: average the three world-space normals and require the
        // result to point up (toward the top-down camera, +z).
        let avg_nz: f32 = tri
            .iter()
            .map(|&i| {
                let v = &read.vertices[i].vertex;
                transform_dir_z(&m, v.nx, v.ny, v.nz)
            })
            .sum::<f32>()
            / 3.0;
        if avg_nz <= 0.0 {
            continue;
        }

        let base = positions.len() as u32;
        for &i in &tri {
            let v = &read.vertices[i].vertex;
            let w = transform_point(&m, v.x, v.y, v.z);
            min_z = min_z.min(w[2]);
            max_z = max_z.max(w[2]);
            // vpx world -> bevy: x right, y up (negated), z = render height.
            positions.push([vpu_to_m(w[0]), -vpu_to_m(w[1]), vpu_to_m(w[2])]);
            uvs.push([v.tu, v.tv]);
        }
        indices.extend_from_slice(&[base, base + 1, base + 2]);
    }

    if positions.is_empty() {
        return None;
    }

    // Rebase vertex z around the centre height (vpinball sorts parts by their bounding
    // sphere centre); the offsets keep face order within the mesh.
    let center_z_vpu = (min_z + max_z) * 0.5;
    let center_z_m = vpu_to_m(center_z_vpu);
    for p in &mut positions {
        p[2] -= center_z_m;
    }

    crate::vpx::ramp_mesh::sort_triangles_by_height(&positions, &mut indices);

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    Some((
        mesh,
        crate::vpx::assets::MeshHeights {
            center: center_z_vpu,
            base: min_z,
        },
    ))
}

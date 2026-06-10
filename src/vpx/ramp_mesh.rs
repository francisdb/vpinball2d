//! 2D (top-down) ramp mesh generation.
//!
//! VPinball ramps are 3D structures (a curved floor with optional side walls, or a set
//! of wires). This project renders the table top-down, so we only need each ramp's
//! silhouette: the floor band for flat ramps, or thin ribbons following each wire for
//! wire ramps.
//!
//! The outline maths are ported from vpinball's `Ramp::GetRampVertex` (ramp.cpp), via
//! vpin's `vpx::mesh::ramps` (which is `pub(crate)` there, so we re-derive the 2D part
//! here): walk the smoothed centerline, build a mitred normal at each point, then offset
//! left/right by the interpolated width. Heights are interpolated linearly along the
//! curve and used only as the render z (depth ordering), since physics stays in 2D.

use bevy::asset::RenderAssetUsages;
use bevy::math::Vec2;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use vpin::vpx::gameitem::ramp::{Ramp, RampType};
use vpin::vpx::gameitem::ramp_image_alignment::RampImageAlignment;
use vpin::vpx::units::vpu_to_m;

/// Per-centerline-point data derived from the smoothed drag points.
struct RampSpine {
    /// Smoothed centerline points (vpx units).
    mid: Vec<Vec2>,
    /// Mitred unit-ish normal at each centerline point (vpx units).
    normal: Vec<Vec2>,
    /// Interpolated surface height at each point (vpx units).
    height: Vec<f32>,
}

/// Walk the centerline and compute the mitred normal and interpolated height at each
/// point. Ports the relevant part of vpinball's `Ramp::GetRampVertex`.
fn ramp_spine(mid: Vec<Vec2>, height_bottom: f32, height_top: f32) -> RampSpine {
    let n = mid.len();
    let mut normal = vec![Vec2::ZERO; n];
    let mut height = vec![0.0f32; n];

    // Approximate the total length of the centerline (ramps are open, they don't loop).
    let mut total_length = 0.0f32;
    for i in 0..n - 1 {
        total_length += mid[i].distance(mid[i + 1]);
    }

    let mut current_length = 0.0f32;
    for i in 0..n {
        // Clamp neighbours: ramps do not loop.
        let vprev = mid[if i > 0 { i - 1 } else { i }];
        let vnext = mid[if i < n - 1 { i + 1 } else { i }];
        let vmiddle = mid[i];

        // Edge normals on either side of this point (rotate the edge by 90 degrees).
        let v1normal = Vec2::new(vprev.y - vmiddle.y, vmiddle.x - vprev.x);
        let v2normal = Vec2::new(vmiddle.y - vnext.y, vnext.x - vmiddle.x);

        normal[i] = if i == n - 1 {
            v1normal.normalize_or_zero()
        } else if i == 0 {
            v2normal.normalize_or_zero()
        } else {
            let v1n = v1normal.normalize_or_zero();
            let v2n = v2normal.normalize_or_zero();
            if (v1n.x - v2n.x).abs() < 0.0001 && (v1n.y - v2n.y).abs() < 0.0001 {
                // Two parallel segments: either normal works.
                v1n
            } else {
                // Mitre: intersect the two offset edges meeting at this point so the band
                // keeps a constant width through corners (vpinball ramp.cpp).
                let a = vprev.y - vmiddle.y;
                let b = vmiddle.x - vprev.x;
                let c = a * (v1n.x - vprev.x) + b * (v1n.y - vprev.y);

                let d = vnext.y - vmiddle.y;
                let e = vmiddle.x - vnext.x;
                let f = d * (v2n.x - vnext.x) + e * (v2n.y - vnext.y);

                let det = a * e - b * d;
                let inv_det = if det != 0.0 { 1.0 / det } else { 0.0 };
                let intersect_x = (b * f - e * c) * inv_det;
                let intersect_y = (c * d - a * f) * inv_det;
                Vec2::new(vmiddle.x - intersect_x, vmiddle.y - intersect_y)
            }
        };

        current_length += vprev.distance(vmiddle);
        let percentage = if total_length > 0.0 {
            current_length / total_length
        } else {
            0.0
        };
        height[i] = percentage * (height_top - height_bottom) + height_bottom;
    }

    RampSpine {
        mid,
        normal,
        height,
    }
}

/// The per-point half-width to offset the band edges by, in vpx units.
fn half_width_at(ramp: &Ramp, percentage: f32) -> f32 {
    let width = match ramp.ramp_type {
        RampType::Flat => percentage * (ramp.width_top - ramp.width_bottom) + ramp.width_bottom,
        RampType::OneWire => ramp.wire_diameter,
        _ => ramp.wire_distance_x,
    };
    width * 0.5
}

/// Append a quad strip (a band) between two edge polylines to the mesh buffers.
/// `edge_a`/`edge_b` are matched-length lists of (vpx-space) points; `z` gives the
/// render height per point (depth ordering); `uv_a`/`uv_b` the per-point texture
/// coordinates of each edge.
fn append_band(
    positions: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    edge_a: &[Vec2],
    edge_b: &[Vec2],
    z: &[f32],
    uv_a: &[[f32; 2]],
    uv_b: &[[f32; 2]],
) {
    let base = positions.len() as u32;
    let n = edge_a.len();
    for i in 0..n {
        positions.push([vpu_to_m(edge_a[i].x), -vpu_to_m(edge_a[i].y), z[i]]);
        positions.push([vpu_to_m(edge_b[i].x), -vpu_to_m(edge_b[i].y), z[i]]);
        uvs.push(uv_a[i]);
        uvs.push(uv_b[i]);
    }
    for i in 0..n - 1 {
        let a = base + (i as u32) * 2;
        // (a, a+1, a+3) and (a, a+3, a+2)
        indices.extend_from_slice(&[a, a + 1, a + 3, a, a + 3, a + 2]);
    }
}

/// Build the top-down ramp mesh, or `None` if the ramp is degenerate.
///
/// `centerline` is the smoothed drag-point centerline in vpx units (open, not looped).
pub fn build_ramp_mesh_2d(table_size: Vec2, ramp: &Ramp, centerline: Vec<Vec2>) -> Option<Mesh> {
    if centerline.len() < 2 {
        return None;
    }
    if ramp.width_bottom == 0.0 && ramp.width_top == 0.0 && !is_wire(ramp) {
        return None;
    }

    let spine = ramp_spine(centerline, ramp.height_bottom, ramp.height_top);
    let n = spine.mid.len();
    let total_length: f32 = (0..n - 1)
        .map(|i| spine.mid[i].distance(spine.mid[i + 1]))
        .sum();
    // Vertex z relative to the ramp's top height: the spawner puts that height into the
    // entity transform (transparent 2D sorting only sees the transform, like walls), so
    // the per-vertex offsets only refine depth within the ramp for the opaque path.
    let z_top = ramp.height_bottom.max(ramp.height_top);
    let z: Vec<f32> = spine.height.iter().map(|h| vpu_to_m(h - z_top)).collect();

    // Offset polyline at the given signed multiple of the half width.
    let offset = |sign: f32, width_scale: f32| -> Vec<Vec2> {
        let mut cur = 0.0f32;
        (0..n)
            .map(|i| {
                if i > 0 {
                    cur += spine.mid[i - 1].distance(spine.mid[i]);
                }
                let pct = if total_length > 0.0 {
                    cur / total_length
                } else {
                    0.0
                };
                let hw = half_width_at(ramp, pct) * width_scale;
                spine.mid[i] + spine.normal[i] * (sign * hw)
            })
            .collect()
    };

    // Lengthwise wrap v at each centerline point, vpinball's `rgratio`
    // (`GetRampVertex`: 1 at the first drag point, falling to 0 at the last).
    let ratio: Vec<f32> = {
        let mut cur = 0.0f32;
        (0..n)
            .map(|i| {
                if i > 0 {
                    cur += spine.mid[i - 1].distance(spine.mid[i]);
                }
                if total_length > 0.0 {
                    1.0 - cur / total_length
                } else {
                    0.0
                }
            })
            .collect()
    };
    // World-aligned UVs: each vertex samples the image stretched over the whole table,
    // like wall tops (vpinball `Ramp::ExportMesh`, ImageModeWorld).
    let world_uv = |edge: &[Vec2]| -> Vec<[f32; 2]> {
        edge.iter()
            .map(|p| [p.x / table_size.x, p.y / table_size.y])
            .collect()
    };

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    if is_wire(ramp) {
        // Wire ramps: render a thin ribbon (wire_diameter wide) along each wire path. The
        // wire paths sit at the centerline (1-wire) or at +/- wire_distance_x/2 (others).
        // Upper wires of 3/4-wire ramps overlap their lower wires in top-down view, so we
        // only draw the distinct ground paths.
        let paths: Vec<Vec<Vec2>> = match ramp.ramp_type {
            RampType::OneWire => vec![spine.mid.clone()],
            _ => vec![offset(1.0, 1.0), offset(-1.0, 1.0)],
        };
        for path in &paths {
            // Build thin ribbon edges by offsetting the path along the shared normals.
            let edge_a: Vec<Vec2> = (0..n)
                .map(|i| path[i] + spine.normal[i] * (ramp.wire_diameter * 0.5))
                .collect();
            let edge_b: Vec<Vec2> = (0..n)
                .map(|i| path[i] - spine.normal[i] * (ramp.wire_diameter * 0.5))
                .collect();
            // Wrapped wires sample one image column along the path (vpinball ramp.cpp).
            let (uv_a, uv_b) = if ramp.image_alignment == RampImageAlignment::Wrap {
                let uv: Vec<[f32; 2]> = ratio.iter().map(|r| [0.0, *r]).collect();
                (uv.clone(), uv)
            } else {
                (world_uv(&edge_a), world_uv(&edge_b))
            };
            append_band(
                &mut positions,
                &mut uvs,
                &mut indices,
                &edge_a,
                &edge_b,
                &z,
                &uv_a,
                &uv_b,
            );
        }
    } else {
        // Flat ramp: a single filled floor band from the right edge to the left edge.
        let right = offset(1.0, 1.0);
        let left = offset(-1.0, 1.0);
        // Wrap alignment spans the image across the band (u 1 -> 0) and along its
        // length (v = ratio), e.g. the apron score cards; World stretches it over the
        // table like wall tops (vpinball ramp.cpp, `Ramp::ExportMesh`).
        let (uv_right, uv_left) = if ramp.image_alignment == RampImageAlignment::Wrap {
            (
                ratio.iter().map(|r| [1.0, *r]).collect::<Vec<_>>(),
                ratio.iter().map(|r| [0.0, *r]).collect::<Vec<_>>(),
            )
        } else {
            (world_uv(&right), world_uv(&left))
        };
        append_band(
            &mut positions,
            &mut uvs,
            &mut indices,
            &right,
            &left,
            &z,
            &uv_right,
            &uv_left,
        );
    }

    if positions.is_empty() || indices.is_empty() {
        return None;
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

fn is_wire(ramp: &Ramp) -> bool {
    !matches!(ramp.ramp_type, RampType::Flat)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight flat ramp along +x becomes a rectangular floor band: 4 vertices, 2
    /// triangles, spanning the ramp length in x and the ramp width (centered) in y.
    #[test]
    fn flat_ramp_is_a_centered_band() {
        let ramp = Ramp {
            ramp_type: RampType::Flat,
            width_bottom: 20.0,
            width_top: 20.0,
            height_bottom: 0.0,
            height_top: 0.0,
            ..Default::default()
        };
        let centerline = vec![Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0)];
        let mesh = build_ramp_mesh_2d(Vec2::new(10.0, 20.0), &ramp, centerline).unwrap();

        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        assert_eq!(
            positions.len(),
            4,
            "two centerline points -> 4 band vertices"
        );
        let indices = mesh.indices().unwrap().iter().count();
        assert_eq!(indices, 6, "one quad -> two triangles");

        let half_w = vpu_to_m(10.0); // half of width 20
        let len = vpu_to_m(100.0);
        let xs: Vec<f32> = positions.iter().map(|p| p[0]).collect();
        let ys: Vec<f32> = positions.iter().map(|p| p[1]).collect();
        let min_x = xs.iter().cloned().fold(f32::MAX, f32::min);
        let max_x = xs.iter().cloned().fold(f32::MIN, f32::max);
        let min_y = ys.iter().cloned().fold(f32::MAX, f32::min);
        let max_y = ys.iter().cloned().fold(f32::MIN, f32::max);
        assert!((min_x - 0.0).abs() < 1e-6 && (max_x - len).abs() < 1e-6);
        // Band is centered on the path: +/- half width (y negated into bevy space).
        assert!((min_y + half_w).abs() < 1e-6 && (max_y - half_w).abs() < 1e-6);
    }

    /// A one-wire guide along a curve (like North Pole's `MetalGuide002`) produces a
    /// solid ribbon with real triangles, so it yields a trimesh collider rather than the
    /// degenerate polyline fallback.
    #[test]
    fn one_wire_guide_is_a_solid_ribbon() {
        let ramp = Ramp {
            ramp_type: RampType::OneWire,
            wire_diameter: 6.0,
            height_bottom: 30.0,
            height_top: 30.0,
            ..Default::default()
        };
        // A curved centerline like the MetalGuide002 arc.
        let centerline = vec![
            Vec2::new(752.0, 216.0),
            Vec2::new(817.0, 304.0),
            Vec2::new(856.0, 410.0),
            Vec2::new(862.0, 488.0),
        ];
        let mesh = build_ramp_mesh_2d(Vec2::new(10.0, 20.0), &ramp, centerline).unwrap();
        let verts = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap()
            .len();
        let tris = mesh.indices().unwrap().iter().count() / 3;
        assert_eq!(verts, 8, "one ribbon over 4 points -> 8 vertices");
        assert_eq!(tris, 6, "3 segments x 2 triangles");
    }

    /// A two-wire ramp emits two separate ribbons (one per rail): 8 vertices total.
    #[test]
    fn two_wire_ramp_has_two_ribbons() {
        let ramp = Ramp {
            ramp_type: RampType::TwoWire,
            wire_diameter: 6.0,
            wire_distance_x: 40.0,
            ..Default::default()
        };
        let centerline = vec![Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0)];
        let mesh = build_ramp_mesh_2d(Vec2::new(10.0, 20.0), &ramp, centerline).unwrap();
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        assert_eq!(positions.len(), 8, "two ribbons x 4 vertices each");
    }
}

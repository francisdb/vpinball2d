use avian2d::math::Vector;
use avian2d::prelude::{Collider, CollisionEventsEnabled, Friction, Restitution, RigidBody};
use bevy::asset::{Assets, RenderAssetUsages};
use bevy::color::palettes::css;
use bevy::color::{Color, Srgba};
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::mesh::{Indices, Mesh, Mesh2d, PrimitiveTopology};
use bevy::prelude::*;
use vpin::vpx;
use vpin::vpx::units::vpu_to_m;

const RUBER_COLOR: Srgba = css::WHITE;

#[derive(Component)]
pub struct Rubber {
    #[allow(dead_code)]
    pub name: String,
    /// World-space centre of the rubber band. Used by slingshots to derive the kick
    /// direction from the offset between the rest and flexed (extended) rubbers.
    pub center: Vec2,
}

pub(super) fn spawn_rubber(
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_to_bevy_transform: Transform,
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    rubber: &vpx::gameitem::rubber::Rubber,
) {
    // A rubber is a band (ring) following its drag points with width rubber.thickness,
    // like Visual Pinball - not a filled shape.

    // sometimes rubbers are used to just render a metallic ring without collision
    if rubber.rot_x != 0.0 || rubber.rot_y != 0.0 || rubber.rot_z != 0.0 {
        warn!(
            "Skipping rubber {} with rotation which is not supported yet.",
            rubber.name
        );
        return;
    }

    // Centerline of the band, smoothed with the same Catmull-Rom spline VPX uses for
    // rubber/wall meshes (closed loop, max accuracy 4.0), then converted to bevy coords.
    let centerline: Vec<Vec2> =
        vpin::vpx::mesh::smooth_drag_points_2d(&rubber.drag_points, 4.0, true)
            .iter()
            .map(|(x, y)| Vec2::new(vpu_to_m(*x), -vpu_to_m(*y)))
            .collect();
    if centerline.len() < 3 {
        return;
    }
    // VPX rubber thickness in vpu, defaulting to 8 when unset (matches vpin's mesh code).
    let thickness = if rubber.thickness == 0 {
        8
    } else {
        rubber.thickness
    };
    let half_width = vpu_to_m(thickness as f32) * 0.5;
    // Lift the band above the playfield (at z 0) so it renders on top.
    let top_height = vpu_to_m(rubber.height + thickness as f32 / 2.0);

    // World-space centre of the band (the transform is the table offset).
    let band_center = vpx_to_bevy_transform.translation.truncate()
        + centerline.iter().copied().sum::<Vec2>() / centerline.len() as f32;

    let mesh = meshes.add(rubber_ring_mesh(&centerline, half_width));

    // Collide along the band centerline (a thin closed loop).
    let mut outline: Vec<Vector> = centerline.iter().map(|p| Vector::new(p.x, p.y)).collect();
    outline.push(outline[0]);
    let collider = Collider::polyline(outline, None);

    // Hidden rubbers (e.g. the slingshot's flexed-frame rubbers) are not drawn at rest;
    // they are shown briefly during the slingshot animation (see pinball::wall).
    let visibility = if rubber.is_visible {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };

    parent.spawn((
        Rubber {
            name: rubber.name.clone(),
            center: band_center,
        },
        Name::from(format!("Rubber {}", rubber.name)),
        Transform::from_xyz(
            vpx_to_bevy_transform.translation.x,
            vpx_to_bevy_transform.translation.y,
            top_height,
        ),
        Mesh2d(mesh),
        MeshMaterial2d(materials.add(Color::from(RUBER_COLOR))),
        visibility,
        // physics
        CollisionEventsEnabled,
        RigidBody::Static,
        collider,
        Restitution::from(rubber.elasticity),
        Friction::from(rubber.friction),
    ));
}

/// Build a closed band mesh of the given half width around a closed centerline (treated
/// as cyclic): the centerline offset by +/- half_width, triangulated as a ring strip.
fn rubber_ring_mesh(centerline: &[Vec2], half_width: f32) -> Mesh {
    let n = centerline.len();
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n * 2);
    for i in 0..n {
        let prev = centerline[(i + n - 1) % n];
        let next = centerline[(i + 1) % n];
        let normal = (next - prev).normalize_or_zero().perp();
        positions.push((centerline[i] + normal * half_width).extend(0.0).to_array());
        positions.push((centerline[i] - normal * half_width).extend(0.0).to_array());
    }
    let mut indices: Vec<u32> = Vec::with_capacity(n * 6);
    for i in 0..n as u32 {
        let next = (i + 1) % n as u32;
        let (outer, inner, outer_next, inner_next) = (i * 2, i * 2 + 1, next * 2, next * 2 + 1);
        indices.extend_from_slice(&[outer, inner, outer_next, outer_next, inner, inner_next]);
    }
    let uvs: Vec<[f32; 2]> = positions.iter().map(|_| [0.0, 0.0]).collect();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

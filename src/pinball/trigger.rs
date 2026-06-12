use avian2d::prelude::{Collider, CollisionEventsEnabled, RigidBody, Sensor};
use bevy::asset::RenderAssetUsages;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::mesh::{Indices, Mesh, Mesh2d, PrimitiveTopology};
use bevy::prelude::*;
use vpin::vpx;
use vpin::vpx::units::vpu_to_m;

#[derive(Component)]
pub struct Trigger {
    #[allow(dead_code)]
    pub name: String,
}

/// Trigger wire colour: bare steel.
const WIRE_COLOR: Color = Color::srgb(0.72, 0.72, 0.75);
/// Wires sit just above the playfield (and the insert lights), under the ball.
const WIRE_Z: f32 = 0.003;

pub(super) fn spawn_trigger(
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_to_bevy_transform: Transform,
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    trigger: &vpx::gameitem::trigger::Trigger,
) {
    // TODO triggers in case the shape is None have a custom polygon shape
    let radius = vpu_to_m(trigger.radius);
    let mut entity = parent.spawn((
        Trigger {
            name: trigger.name.clone(),
        },
        Name::from(format!("Trigger {}", trigger.name)),
        Transform::from_xyz(
            vpx_to_bevy_transform.translation.x + vpu_to_m(trigger.center.x),
            vpx_to_bevy_transform.translation.y - vpu_to_m(trigger.center.y),
            WIRE_Z,
        ),
        CollisionEventsEnabled,
        RigidBody::Static,
        Collider::circle(radius),
        Sensor,
    ));
    // Visible triggers draw their vpinball shape (wire loops, buttons, stars)
    // seen from above; invisible ones stay collider-only (the playfield art
    // shows them), visible in the dev collider view.
    if let Some(mesh) = trigger_mesh(trigger) {
        entity.insert((
            Mesh2d(meshes.add(mesh)),
            MeshMaterial2d(materials.add(WIRE_COLOR)),
        ));
    }
}

/// Top-down projection of vpinball's trigger mesh (vpin `build_trigger_mesh`,
/// pivot-centred with rotation and radius baked in): the upward faces only, so
/// the top of the wire reads as a thin band and the underside never overdraws it.
fn trigger_mesh(trigger: &vpx::gameitem::trigger::Trigger) -> Option<Mesh> {
    let (verts, faces) = vpin::vpx::mesh::triggers::build_trigger_mesh(trigger)?;
    let positions: Vec<[f32; 3]> = verts
        .iter()
        .map(|v| [vpu_to_m(v.vertex.x), -vpu_to_m(v.vertex.y), 0.0])
        .collect();
    let mut indices: Vec<u32> = Vec::new();
    for face in &faces {
        // Keep the faces that look up: after projecting to 2D, an upward face
        // keeps a consistent winding, so the signed area's sign separates the
        // top of the wire from its underside (the vertex normals of the wire
        // tube point sideways, so they cannot tell the two apart).
        let tri = [face.i0 as usize, face.i2 as usize, face.i1 as usize];
        let p = |i: usize| Vec2::new(positions[i][0], positions[i][1]);
        let (a, b, c) = (p(tri[0]), p(tri[1]), p(tri[2]));
        let area = (b - a).perp_dot(c - a);
        if area > 0.0 {
            indices.extend(tri.map(|i| i as u32));
        }
    }
    if indices.is_empty() {
        return None;
    }
    let uvs = vec![[0.0, 0.0]; positions.len()];
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

// TODO handle ball-trigger collisions

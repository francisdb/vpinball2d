use crate::PausableSystems;
use crate::pinball::ball::Ball;
use crate::pinball::table::TableAssets;
use crate::vpx::VpxAsset;
use avian2d::math::Scalar;
use avian2d::prelude::*;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;

use crate::audio::play_sound_at;
use crate::screens::Screen;
use bevy::sprite_render::AlphaMode2d;
use vpin::vpx::gameitem;
use vpin::vpx::gameitem::GameItemEnum;
use vpin::vpx::gameitem::primitive::Primitive;
use vpin::vpx::units::vpu_to_m;

/// A bumper cap is a textured primitive placed (almost exactly) on the bumper centre.
/// Within this distance (vpx units) a visible textured primitive is treated as the cap.
const BUMPER_CAP_MAX_DIST_VPU: f32 = 30.0;

/// The textured primitive sitting on a bumper (its cap), if any. Tables hide the built-in
/// cap (`is_cap_visible = false`) and place a textured primitive there; we render that flat
/// (a textured disc) rather than as a distorted top-down dome projection.
pub(super) fn cap_primitive_for<'a>(
    gameitems: &'a [GameItemEnum],
    bumper: &gameitem::bumper::Bumper,
) -> Option<&'a Primitive> {
    let center = &bumper.center;
    let dist2 =
        |p: &Primitive| (p.position.x - center.x).powi(2) + (p.position.y - center.y).powi(2);
    gameitems
        .iter()
        .filter_map(|it| match it {
            GameItemEnum::Primitive(p) if p.is_visible && !p.image.is_empty() => Some(p),
            _ => None,
        })
        .filter(|p| dist2(p) < BUMPER_CAP_MAX_DIST_VPU.powi(2))
        .min_by(|a, b| dist2(a).total_cmp(&dist2(b)))
}

/// Whether a primitive is a bumper cap, so the general primitive renderer can skip it (the
/// bumper renders it flat instead).
pub(crate) fn is_bumper_cap(gameitems: &[GameItemEnum], primitive: &Primitive) -> bool {
    if !primitive.is_visible || primitive.image.is_empty() {
        return false;
    }
    gameitems.iter().any(|it| {
        matches!(it, GameItemEnum::Bumper(b)
            if (b.center.x - primitive.position.x).powi(2)
                + (b.center.y - primitive.position.y).powi(2)
                < BUMPER_CAP_MAX_DIST_VPU.powi(2))
    })
}

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        handle_bumper_collisions
            .in_set(PausableSystems)
            .run_if(in_state(Screen::Gameplay)),
    );
}

#[derive(Component)]
struct Bumper {
    force: Scalar,
}

/// Sounds a table plays when a bumper is hit. A random entry is picked. A table enables
/// bumper sounds by inserting this resource.
#[derive(Resource, Default)]
pub struct BumperSounds {
    pub hit: Vec<String>,
}

pub(super) fn spawn_bumper(
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_asset: &VpxAsset,
    vpx_to_bevy_transform: Transform,
    bumper: &gameitem::bumper::Bumper,
) {
    // TODO we might want to create the mesh in the asset loader instead
    let base_radius = vpu_to_m(bumper.radius);
    // TODO check how big the default cap is in vpinball
    let cap_radius = base_radius + 0.015;
    let mesh = Mesh::from(Circle {
        radius: vpu_to_m(bumper.radius),
    });
    let vpx_base_material_color = if bumper.base_material.is_empty() {
        Srgba::rgb_u8(150, 150, 150)
    } else {
        match vpx_asset
            .raw
            .gamedata
            .materials
            .iter()
            .flatten()
            .find(|m| m.name == bumper.base_material)
        {
            None => {
                warn!(
                    "Bumper base material '{}' not found, using default color",
                    bumper.base_material
                );
                Srgba::rgb_u8(150, 150, 150)
            }
            Some(m) => {
                let base_color = m.base_color;
                Srgba::rgb_u8(base_color.r, base_color.g, base_color.b)
            }
        }
    };

    let base_material = materials.add(ColorMaterial {
        color: vpx_base_material_color.into(),
        alpha_mode: AlphaMode2d::Opaque,
        texture: None,
        ..default()
    });

    // use bumper.center to modify the transform
    let transform = Transform::from_xyz(
        vpu_to_m(bumper.center.x) + vpx_to_bevy_transform.translation.x,
        -vpu_to_m(bumper.center.y) + vpx_to_bevy_transform.translation.y,
        0.1,
    );
    // not sure what vpinball uses as force but we want newtons
    let force = bumper.force * 0.008;
    let mut entity = parent.spawn((
        Bumper { force },
        Name::from(format!("Bumper{}", bumper.name)),
        Mesh2d(meshes.add(mesh)),
        MeshMaterial2d(base_material),
        transform,
        CollisionEventsEnabled,
        RigidBody::Static,
        Collider::circle(base_radius),
        // Drop a shadow into the light map; scale it past the wider cap so it shows.
        crate::pinball::light::ShadowCaster {
            scale: (cap_radius / base_radius) * 1.3,
        },
    ));
    // Most tables hide the built-in cap (`is_cap_visible = false`) and place a textured
    // cap primitive on the bumper. We draw that as a flat textured disc here: a cap reads
    // as its flat art from straight above, whereas projecting its little dome top-down
    // loses the centre artwork. The primitive renderer skips caps (see `is_bumper_cap`) so
    // they are not drawn twice. Tables that keep the built-in cap fall back to a flat
    // colour from the cap material.
    if let Some(cap) = cap_primitive_for(&vpx_asset.raw.gameitems, bumper) {
        // The cap art is a round image on a square texture; a disc clips the corners.
        let cap_tex_radius = vpu_to_m(bumper.radius) * 1.6;
        let cap_material = materials.add(ColorMaterial {
            color: Color::WHITE,
            alpha_mode: AlphaMode2d::Blend,
            texture: vpx_asset.image(cap.image.as_str()).cloned(),
            ..default()
        });
        let cap_mesh = meshes.add(Mesh::from(Circle {
            radius: cap_tex_radius,
        }));
        entity.with_children(|parent| {
            parent.spawn((
                Name::from(format!("Bumper Cap {}", bumper.name)),
                Mesh2d(cap_mesh),
                MeshMaterial2d(cap_material),
                Transform::from_xyz(0.0, 0.0, 0.01),
            ));
        });
    } else if bumper.is_cap_visible {
        let cap_color = bumper
            .cap_material
            .is_empty()
            .then_some(Srgba::rgb_u8(200, 200, 200))
            .or_else(|| {
                vpx_asset
                    .raw
                    .gamedata
                    .materials
                    .iter()
                    .flatten()
                    .find(|m| m.name == bumper.cap_material)
                    .map(|m| Srgba::rgb_u8(m.base_color.r, m.base_color.g, m.base_color.b))
            })
            .unwrap_or(Srgba::rgb_u8(200, 200, 200));
        let cap_material = materials.add(ColorMaterial {
            color: cap_color.into(),
            // TODO we want to create a proper transparent plastic material type
            alpha_mode: AlphaMode2d::Blend,
            texture: None,
            ..default()
        });
        let cap_mesh = meshes.add(Mesh::from(Circle { radius: cap_radius }));
        entity.with_children(|parent| {
            parent.spawn((
                Name::from(format!("Bumper Cap {}", bumper.name)),
                Mesh2d(cap_mesh),
                MeshMaterial2d(cap_material),
                Transform::from_xyz(0.0, 0.0, 0.01),
            ));
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_bumper_collisions(
    bumper_query: Query<(Entity, &Bumper, &Transform)>,
    mut ball_query: Query<(&Transform, Forces), With<Ball>>,
    mut contact_events: MessageReader<CollisionStart>,
    mut commands: Commands,
    sounds: Option<Res<BumperSounds>>,
    table_assets: Res<TableAssets>,
    assets_vpx: Res<Assets<VpxAsset>>,
) {
    for contact_event in contact_events.read() {
        for (bumper_entity, bumper, bumper_transform) in bumper_query.iter() {
            if let (Some(h1), Some(h2)) = (contact_event.body1, contact_event.body2)
                && (h1 == bumper_entity || h2 == bumper_entity)
            {
                // play a bumper hit sound at the bumper
                if let Some(sounds) = &sounds {
                    play_sound_at(
                        &mut commands,
                        &table_assets,
                        &assets_vpx,
                        bumper_entity,
                        &sounds.hit,
                    );
                }

                // Apply outward pulse to the ball
                let ball_entity = if h1 == bumper_entity { h2 } else { h1 };
                if let Ok((ball_transform, mut forces)) = ball_query.get_mut(ball_entity) {
                    // Calculate direction from bumper center to ball
                    let bumper_pos = bumper_transform.translation.truncate();
                    let ball_pos = ball_transform.translation.truncate();
                    let direction = (ball_pos - bumper_pos).normalize();

                    forces.apply_linear_impulse(direction * bumper.force);
                }
            }
        }
    }
}

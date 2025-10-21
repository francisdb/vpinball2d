use avian2d::prelude::*;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::{AppSystems, PausableSystems, asset_tracking::LoadResource};

// A typical pinball ball is
// 1-1/16 inches (27 mm) in diameter
const BALL_RADIUS_M: f32 = 0.027;

// A typical pinball ball mass is around 80 grams
const BALL_MASS_KG: f32 = 0.08;

#[derive(Component)]
pub struct Ball;

pub(super) fn plugin(app: &mut App) {
    app.load_resource::<BallAssets>();

    // Mouse ball control for development purposes
    app.add_systems(
        Update,
        mouse_ball_control
            .in_set(AppSystems::RecordInput)
            .in_set(PausableSystems),
    );
}

pub(crate) fn ball(
    ball_assets: &BallAssets,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> impl Bundle {
    let ball_material = materials.add(ColorMaterial {
        texture: Some(ball_assets.image.clone()),
        ..default()
    });
    (
        Ball,
        Mesh2d::from(meshes.add(Mesh::from(Circle::new(BALL_RADIUS_M)))),
        MeshMaterial2d::from(ball_material),
        RigidBody::Dynamic,
        Mass::from(BALL_MASS_KG),
        Restitution::new(0.4),
        Friction::from(0.2),
        Collider::circle(BALL_RADIUS_M),
        SleepingDisabled,
        ConstantForce::default(),
    )
}

fn mouse_ball_control(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    gravity: Res<Gravity>,
    mut ball_query: Query<(&Transform, &mut LinearVelocity, &mut ConstantForce), With<Ball>>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
) {
    // TODO get rid of the ugly unwrap
    let (camera, camera_transform) = camera_query.single().unwrap();

    if mouse_buttons.pressed(MouseButton::Left) {
        // check if the cursor is inside the window and get its position
        // then, ask bevy to convert into world coordinates, and truncate to discard Z
        if let Some(world_position) = window
            .cursor_position()
            .and_then(|cursor| camera.viewport_to_world(camera_transform, cursor).ok())
            .map(|ray| ray.origin.truncate())
        {
            for (transform, mut velocity, mut constant_force) in ball_query.iter_mut() {
                let ball_pos = transform.translation.truncate();
                let direction = (world_position - ball_pos).normalize_or_zero();
                let distance = world_position.distance(ball_pos);
                // adjust ball velocity towards the mouse position
                let strength = 8.0;
                velocity.0 = direction * distance * strength;
                // cancel gravity
                constant_force.0 = -gravity.0 * BALL_MASS_KG;
            }
        }
    } else {
        for (_transform, _velocity, mut constant_force) in ball_query.iter_mut() {
            // keep velocity so we can sling the ball around but cancel the anti-gravity force
            constant_force.0 = Vec2::ZERO;
        }
    }
}

#[derive(Resource, Asset, Clone, Reflect)]
#[reflect(Resource)]
pub struct BallAssets {
    #[dependency]
    image: Handle<Image>,
    // #[dependency]
    // pub steps: Vec<Handle<AudioSource>>,
}

impl FromWorld for BallAssets {
    fn from_world(world: &mut World) -> Self {
        let assets = world.resource::<AssetServer>();
        Self {
            image: assets.load("images/JPBall-Dark2.png"),
            // TODO add ball rolling sound effects
            // TODO add ball collection sound effects

            // steps: vec![
            //     assets.load("audio/sound_effects/step1.ogg"),
            //     assets.load("audio/sound_effects/step2.ogg"),
            //     assets.load("audio/sound_effects/step3.ogg"),
            //     assets.load("audio/sound_effects/step4.ogg"),
            // ],
        }
    }
}

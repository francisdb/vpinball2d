use avian2d::prelude::*;
use bevy::prelude::*;

use crate::{
    AppSystems, PausableSystems,
    asset_tracking::LoadResource,
    pinball::{
        animation::PlayerAnimation,
        movement::{MovementController, ScreenWrap},
    },
};

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

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
    mut materials: &mut ResMut<Assets<ColorMaterial>>,
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
    // input: Res<ButtonInput<KeyCode>>,
    // mut controller_query: Query<&mut MovementController, With<Table>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    gravity: Res<Gravity>,
    mut ball_query: Query<(&Transform, &mut LinearVelocity, &mut ConstantForce), With<Ball>>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
) {
    // when left mouse button is held down, give the ball a force towards the mouse position

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
                let strength = 5.0; // adjust this value to change the strength of the
                velocity.0 = direction * distance * strength;
                // cancel gravity effect
                constant_force.0 = -gravity.0 * BALL_MASS_KG;
            }
        }
    } else {
        for (_transform, mut velocity, mut constant_force) in ball_query.iter_mut() {
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

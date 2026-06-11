//! Remote control: read commands from [`CMD_PATH`] each frame and inject them as keyboard / ball
//! input, so an operator who cannot see the pixels can drive the game. Commands: `tp`, `launch`,
//! `clear`, flipper/plunger `hold`/`release`/`tap`, `nudge`, and `screenshot [path]` (save the
//! current frame so the operator can see it). Enabled by the `remote_control` feature.

use crate::pinball::ball::Ball;
use crate::screens::Screen;
use avian2d::prelude::*;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use std::fs;

/// File polled for commands; written by the operator, truncated by us once read.
const CMD_PATH: &str = "/tmp/vpinball2d_cmd";

pub(super) fn plugin(app: &mut App) {
    info!("Remote control enabled: cmd={CMD_PATH}");
    app.init_resource::<PendingReleases>();
    app.add_systems(
        Update,
        // Also during Loading, so e.g. `screenshot` can capture the loading screen.
        (read_commands, apply_pending_releases)
            .run_if(in_state(Screen::Gameplay).or(in_state(Screen::Loading))),
    );
}

/// Keys pressed by a `tap` command, paired with the time (elapsed secs) to release them.
#[derive(Resource, Default)]
struct PendingReleases(Vec<(KeyCode, f32)>);

/// Map a control target name to the key the gameplay input systems listen for.
fn key_for(name: &str) -> Option<KeyCode> {
    match name {
        "left" => Some(KeyCode::ArrowLeft),
        "right" => Some(KeyCode::ArrowRight),
        // The plunger systems listen for Enter (see pinball::plunger), not Space.
        "plunge" | "plunger" => Some(KeyCode::Enter),
        // Table-script keys (see scripting::key_code): coin in, start game.
        "coin" => Some(KeyCode::Digit5),
        "start" => Some(KeyCode::Digit1),
        _ => None,
    }
}

/// Map a nudge direction to the key the nudge system listens for (Visual Pinball defaults,
/// see pinball::nudge). `bottom` is the front-of-table nudge that jolts it upward.
fn nudge_key_for(dir: &str) -> Option<KeyCode> {
    match dir {
        "left" => Some(KeyCode::KeyZ),
        "right" => Some(KeyCode::Slash),
        "bottom" => Some(KeyCode::Space),
        _ => None,
    }
}

/// Release any `tap`-pressed keys whose hold time has elapsed.
fn apply_pending_releases(
    time: Res<Time>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    mut pending: ResMut<PendingReleases>,
) {
    let now = time.elapsed_secs();
    pending.0.retain(|(key, release_at)| {
        if now >= *release_at {
            keyboard.release(*key);
            false
        } else {
            true
        }
    });
}

/// Read and execute queued commands, then truncate the command file.
fn read_commands(
    mut commands: Commands,
    time: Res<Time>,
    balls: Query<Entity, With<Ball>>,
    // Teleport moves existing balls (position + velocity) without respawning them. The `Ball`
    // is read so `tp <id> ...` can target a single ball in a multiball test.
    mut ball_bodies: Query<(&Ball, &mut Transform, &mut LinearVelocity)>,
    // Flipper/plunger control works by injecting into the same keyboard resource the
    // gameplay input systems read, so they react exactly as to a real key.
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    mut pending: ResMut<PendingReleases>,
    // In headless mode the main view renders to this image instead of a window.
    headless_image: Option<Res<crate::HeadlessImage>>,
) {
    let Ok(contents) = fs::read_to_string(CMD_PATH) else {
        return;
    };
    if contents.trim().is_empty() {
        return;
    }
    // Consume the file so each command runs once.
    let _ = fs::write(CMD_PATH, "");

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        match tokens.as_slice() {
            ["tp", x, y, vx, vy] => {
                let (Ok(x), Ok(y), Ok(vx), Ok(vy)) = (
                    x.parse::<f32>(),
                    y.parse::<f32>(),
                    vx.parse::<f32>(),
                    vy.parse::<f32>(),
                ) else {
                    warn!("play: bad `tp` command: {line}");
                    continue;
                };
                // Teleport every existing ball to (x,y) with velocity (vx,vy). For a
                // single-ball test this gives a clean, exactly repeatable shot. Does nothing
                // if no ball is in play.
                let mut moved = 0;
                for (_ball, mut transform, mut velocity) in &mut ball_bodies {
                    transform.translation.x = x;
                    transform.translation.y = y;
                    velocity.0 = Vec2::new(vx, vy);
                    moved += 1;
                }
                if moved == 0 {
                    warn!("play: `tp` but no ball in play");
                } else {
                    info!("play: teleported {moved} ball(s) to ({x},{y}) vel ({vx},{vy})");
                }
            }
            ["tp", id, x, y, vx, vy] => {
                let (Ok(id), Ok(x), Ok(y), Ok(vx), Ok(vy)) = (
                    id.parse::<u32>(),
                    x.parse::<f32>(),
                    y.parse::<f32>(),
                    vx.parse::<f32>(),
                    vy.parse::<f32>(),
                ) else {
                    warn!("play: bad `tp` command: {line}");
                    continue;
                };
                // Teleport only the ball with this id, leaving any others untouched, so a
                // multiball test can place each ball independently.
                let mut moved = false;
                for (ball, mut transform, mut velocity) in &mut ball_bodies {
                    if ball.id == id {
                        transform.translation.x = x;
                        transform.translation.y = y;
                        velocity.0 = Vec2::new(vx, vy);
                        moved = true;
                    }
                }
                if moved {
                    info!("play: teleported ball {id} to ({x},{y}) vel ({vx},{vy})");
                } else {
                    warn!("play: `tp` no ball with id {id}");
                }
            }
            ["launch"] | ["launch", _] => {
                let speed = tokens
                    .get(1)
                    .and_then(|s| s.parse::<f32>().ok())
                    .unwrap_or(0.5);
                // Drop every ball into open playfield from the upper centre, with a gentle
                // downward speed, putting it straight into live play. The ball-release lane
                // holds a ball too firmly for an injected velocity or the weak plunger stroke
                // to clear it, so this is the reliable "start a rally" command and replaces
                // faking a relaunch with `tp`. The spawn point is tuned for the demo table.
                let spawn = Vec2::new(0.0, 0.45);
                let mut launched = 0;
                for (_ball, mut transform, mut velocity) in &mut ball_bodies {
                    transform.translation.x = spawn.x;
                    transform.translation.y = spawn.y;
                    velocity.0 = Vec2::new(0.0, -speed);
                    launched += 1;
                }
                if launched == 0 {
                    warn!("play: `launch` but no ball in play");
                } else {
                    info!("play: launched {launched} ball(s) into play at {speed} m/s");
                }
            }
            ["clear"] => {
                info!("play: clearing {} balls", balls.iter().count());
                for entity in &balls {
                    commands.entity(entity).despawn();
                }
            }
            // Flipper/plunger control. `hold`/`release` are momentary-off latches; `tap`
            // presses now and auto-releases after `ms` (default 120). Targets: left, right
            // (flippers), plunge (plunger).
            ["hold", target] => match key_for(target) {
                Some(key) => {
                    keyboard.press(key);
                    info!("play: hold {target}");
                }
                None => warn!("play: unknown target for hold: {target}"),
            },
            ["release", target] => match key_for(target) {
                Some(key) => {
                    keyboard.release(key);
                    info!("play: release {target}");
                }
                None => warn!("play: unknown target for release: {target}"),
            },
            ["tap", target] | ["tap", target, _] => {
                let Some(key) = key_for(target) else {
                    warn!("play: unknown target for tap: {target}");
                    continue;
                };
                let ms = tokens
                    .get(2)
                    .and_then(|s| s.parse::<f32>().ok())
                    .unwrap_or(120.0);
                keyboard.press(key);
                pending.0.push((key, time.elapsed_secs() + ms / 1000.0));
                info!("play: tap {target} for {ms}ms");
            }
            // Nudge is a one-shot impulse (the nudge system reads `just_pressed`), so press the
            // key and auto-release it shortly after, leaving it ready for the next nudge.
            ["nudge", dir] => {
                let Some(key) = nudge_key_for(dir) else {
                    warn!("play: unknown nudge direction: {dir}");
                    continue;
                };
                keyboard.press(key);
                pending.0.push((key, time.elapsed_secs() + 0.05));
                info!("play: nudge {dir}");
            }
            // Save a screenshot of the current frame. Captures the offscreen image in
            // headless mode, otherwise the primary window.
            ["screenshot"] | ["screenshot", _] => {
                let path = tokens.get(1).copied().unwrap_or("/tmp/vpinball2d_shot.png");
                // The save observer outlives `contents`, so it must own the path.
                let owned_path = std::path::PathBuf::from(path);
                let screenshot = match &headless_image {
                    Some(image) => Screenshot(RenderTarget::Image(image.0.clone().into())),
                    None => Screenshot::primary_window(),
                };
                commands.spawn(screenshot).observe(save_to_disk(owned_path));
                info!("play: screenshot -> {path}");
            }
            other => warn!("play: unknown command: {other:?}"),
        }
    }
}

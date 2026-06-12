//! Table scripting: game rules in a sidecar script next to the `.vpx`.
//!
//! Visual Pinball tables ship VBScript we do not run. Instead a table can put a
//! translated script next to its vpx file (`<table>.lua`; the bridge is
//! engine-agnostic, so a `.js` QuickJS binding can slot in later - see
//! [`api::ScriptEngine`]). The script implements the *game rules* - scoring,
//! lights, credits, ball lifecycle - while static feedback (hit sounds,
//! slingshot animations) stays declarative in the engine, configured by the
//! `<table>.table.json` sidecar (see [`sidecar`]); the split follows
//! vpinball#2263.
//!
//! Event names dispatched to the script are lowercase: `table_init`,
//! `table_keydown(code)` / `table_keyup(code)`, `<item>_hit` / `<item>_unhit`,
//! `<item>_slingshot`, `<item>_spin` and `<timer>_timer`.

// On wasm the Lua engine does not build (vendored C sources), so it is gated out
// and the whole script runtime below becomes unreachable - only the Lua path
// ever constructs the host state, commands and engine. That is intentional
// target gating, not stray dead code, so allow it on wasm where `-Dwarnings`
// would otherwise reject the unbuilt runtime.
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

pub mod api;
#[cfg(not(target_arch = "wasm32"))]
mod lua;
mod scoreboard;
pub mod sidecar;

use crate::pinball::TablePath;
use crate::pinball::ball::{Ball, BallAssets, BallMaterial, ball as ball_bundle};
use crate::pinball::kicker::Kicker;
use crate::pinball::light::{Light, LightAnimation};
use crate::pinball::table::TableAssets;
use crate::screens::Screen;
use crate::vpx::VpxAsset;
use api::{HostState, ItemKind, ItemState, ScriptCommand, ScriptEngine, ScriptValue, SharedHost};
use avian2d::prelude::{CollisionEnd, CollisionStart, LinearVelocity, RigidBody};
use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use std::path::PathBuf;
use std::rc::Rc;
use vpin::vpx::gameitem::GameItemEnum;

/// Marks the collider entity of a table item the script can receive hit events
/// from; attached by the item spawners.
#[derive(Component)]
pub struct ScriptName(pub String);

/// Present while a table script is running; gates the script systems and turns
/// off engine behaviours the script takes over (auto ball release, the
/// all-inserts attract blinker).
#[derive(Resource)]
pub struct ScriptActive;

/// Script-controlled flipper gate (a tilted EM cuts flipper power). Treated as
/// enabled when absent.
#[derive(Resource)]
pub struct FlippersEnabled(pub bool);

/// A slingshot fired (written by the wall module), forwarded to the script as
/// `<name>_slingshot`.
#[derive(Message)]
pub struct SlingshotFired {
    pub name: String,
}

/// A spinner passed half a rotation (written by the spinner module), forwarded
/// to the script as `<name>_spin`.
#[derive(Message)]
pub struct SpinnerSpun {
    pub name: String,
}

/// Lowercase sound name -> source, for the script's `playsound` (vbscript is
/// case-insensitive about sound names).
#[derive(Resource, Default)]
struct ScriptSounds(HashMap<String, Handle<AudioSource>>);

/// Tags a playing script sound so `stopsound` can find it.
#[derive(Component)]
struct PlayingSound(String);

/// A ball held in a kicker (saucer or drain), frozen at its centre until the
/// script kicks or destroys it - vpinball's locked-in-kicker state.
#[derive(Component)]
pub struct CapturedBall {
    kicker: Entity,
}

/// Suppresses re-capture by the kicker a ball was just kicked out of (or
/// created in), until the ball actually leaves its sensor.
#[derive(Component)]
struct KickerEscape {
    kicker: Entity,
}

/// A kick whose target ball did not exist yet (ball spawns are deferred a
/// frame); retried until the ball appears or the tries run out.
struct PendingKick {
    name: String,
    angle: f32,
    speed: f32,
    tries: u32,
}

/// One scripting timer, from a vpx Timer gameitem. Fires `<name>_timer`.
struct ScriptTimer {
    /// Lowercase name, the event prefix.
    lower: String,
    interval_ms: f64,
    enabled: bool,
    elapsed_ms: f64,
}

/// The running script: engine + shared host state + timers. NonSend because
/// script engines are not thread-safe.
pub struct ScriptRuntime {
    engine: Box<dyn ScriptEngine>,
    host: SharedHost,
    timers: Vec<ScriptTimer>,
    store_path: PathBuf,
    pending_kicks: Vec<PendingKick>,
}

impl ScriptRuntime {
    /// Dispatch an event, logging script errors instead of crashing the table.
    fn dispatch(&mut self, event: &str, args: &[ScriptValue]) {
        if let Err(e) = self.engine.dispatch(event, args) {
            warn!("script error in {event}: {e}");
        }
    }
}

pub fn plugin(app: &mut App) {
    app.add_message::<SlingshotFired>();
    app.add_message::<SpinnerSpun>();
    app.add_systems(
        Update,
        (
            capture_balls,
            clear_kicker_escapes,
            forward_keys,
            forward_collisions,
            forward_slingshots,
            forward_spins,
            tick_timers,
            apply_commands,
            save_store,
            scoreboard::update_scoreboard,
        )
            .chain()
            .run_if(in_state(Screen::Gameplay))
            .run_if(resource_exists::<ScriptActive>),
    );
    app.add_systems(OnExit(Screen::Gameplay), teardown_script);
    app.add_plugins(sidecar::plugin);
}

/// The sidecar script path for a table: `<table>.lua` next to the vpx.
fn script_path(tables_dir: &crate::tables::TablesDir, table_path: &TablePath) -> PathBuf {
    tables_dir.0.join(&table_path.path).with_extension("lua")
}

/// Whether the table at `rel_path` (relative to the tables dir) ships a script
/// sidecar; used by the table picker.
pub fn has_script_sidecar(tables_dir: &std::path::Path, rel_path: &str) -> bool {
    tables_dir.join(rel_path).with_extension("lua").is_file()
}

/// Web builds have no script engine (the vendored Lua C sources do not build
/// for wasm); tables run scriptless like before.
#[cfg(target_arch = "wasm32")]
pub fn init_script(world: &mut World) {
    world.remove_resource::<ScriptActive>();
}

/// Loads and starts the table's sidecar script, if it has one. Runs before
/// `spawn_level` so the level spawn can adapt (no auto ball, lights start in
/// their authored state instead of the attract blinker).
#[cfg(not(target_arch = "wasm32"))]
pub fn init_script(world: &mut World) {
    let Some(tables_dir) = world.get_resource::<crate::tables::TablesDir>() else {
        return;
    };
    let Some(table_path) = world.get_resource::<TablePath>() else {
        return;
    };
    let path = script_path(tables_dir, table_path);
    let Ok(source) = std::fs::read_to_string(&path) else {
        // No script for this table.
        world.remove_resource::<ScriptActive>();
        return;
    };
    info!("Loading table script {}", path.display());

    let (host, collections, sounds) = {
        let table_assets = world.resource::<TableAssets>();
        let assets_vpx = world.resource::<Assets<VpxAsset>>();
        let vpx_asset = assets_vpx.get(&table_assets.vpx).unwrap();
        let host = build_host(vpx_asset);
        let collections: Vec<(String, Vec<String>)> = vpx_asset
            .raw
            .collections
            .iter()
            .map(|c| (c.name.clone(), c.items.clone()))
            .collect();
        let sounds: HashMap<String, Handle<AudioSource>> = vpx_asset
            .named_sounds
            .iter()
            .map(|(name, handle)| (name.to_lowercase(), handle.clone()))
            .collect();
        (host, collections, sounds)
    };

    // Per-table persistent store (high scores etc.) next to the vpx.
    let store_path = path.with_extension("store.json");
    if let Ok(json) = std::fs::read_to_string(&store_path)
        && let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&json)
    {
        host.borrow_mut().store = map;
    }

    let timers = host
        .borrow()
        .items
        .values()
        .filter(|item| item.kind == ItemKind::Timer)
        .map(|item| ScriptTimer {
            lower: item.name.to_lowercase(),
            interval_ms: item
                .props
                .get("interval")
                .and_then(|v| v.as_f32())
                .unwrap_or(100.0) as f64,
            enabled: item
                .props
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            elapsed_ms: 0.0,
        })
        .collect();

    let mut engine = match lua::LuaEngine::new(host.clone(), &collections) {
        Ok(engine) => Box::new(engine),
        Err(e) => {
            warn!("table script prelude failed: {e}");
            return;
        }
    };
    if let Err(e) = engine.load(&source) {
        warn!("table script failed to load: {e}");
        return;
    }

    let mut runtime = ScriptRuntime {
        engine,
        host,
        timers,
        store_path,
        pending_kicks: Vec::new(),
    };
    runtime.dispatch("table_init", &[]);

    world.insert_resource(ScriptActive);
    world.insert_resource(FlippersEnabled(true));
    world.insert_resource(ScriptSounds(sounds));
    world.insert_non_send_resource(runtime);
    scoreboard::spawn_scoreboard(world);
}

/// The shadow item registry, seeded from the vpx data.
fn build_host(vpx_asset: &VpxAsset) -> SharedHost {
    let mut host = HostState::default();
    for item in &vpx_asset.raw.gameitems {
        let name = item.name().to_string();
        if name.is_empty() {
            continue;
        }
        let mut state = ItemState {
            kind: ItemKind::Other,
            name: name.clone(),
            props: HashMap::default(),
        };
        match item {
            GameItemEnum::Light(light) => {
                state.kind = ItemKind::Light;
                state.props.insert(
                    "state".into(),
                    ScriptValue::Num(light.state.unwrap_or(0.0) as f64),
                );
            }
            GameItemEnum::Kicker(_) => state.kind = ItemKind::Kicker,
            GameItemEnum::Timer(timer) => {
                state.kind = ItemKind::Timer;
                state
                    .props
                    .insert("enabled".into(), ScriptValue::Bool(timer.timer.is_enabled));
                state.props.insert(
                    "interval".into(),
                    ScriptValue::Int(timer.timer.interval as i64),
                );
            }
            GameItemEnum::Flipper(_) => state.kind = ItemKind::Flipper,
            GameItemEnum::Plunger(_) => state.kind = ItemKind::Plunger,
            GameItemEnum::Wall(_) => state.kind = ItemKind::Wall,
            GameItemEnum::Trigger(_) => state.kind = ItemKind::Trigger,
            GameItemEnum::Bumper(_) => state.kind = ItemKind::Bumper,
            GameItemEnum::Spinner(_) => state.kind = ItemKind::Spinner,
            GameItemEnum::TextBox(textbox) => {
                state.kind = ItemKind::TextBox;
                state
                    .props
                    .insert("text".into(), ScriptValue::Str(textbox.text.clone()));
            }
            GameItemEnum::Reel(_) => {
                state.kind = ItemKind::Reel;
                state.props.insert("value".into(), ScriptValue::Int(0));
            }
            _ => {}
        }
        host.items.insert(name.to_lowercase(), state);
    }
    Rc::new(std::cell::RefCell::new(host))
}

/// Save the persistent store and drop the runtime when leaving the table.
fn teardown_script(world: &mut World) {
    if let Some(runtime) = world.remove_non_send_resource::<ScriptRuntime>() {
        write_store(&runtime);
    }
    world.remove_resource::<ScriptActive>();
    world.remove_resource::<FlippersEnabled>();
    world.remove_resource::<ScriptSounds>();
}

fn write_store(runtime: &ScriptRuntime) {
    let host = runtime.host.borrow();
    if host.store.is_empty() {
        return;
    }
    if let Ok(json) = serde_json::to_string_pretty(&host.store)
        && let Err(e) = std::fs::write(&runtime.store_path, json)
    {
        warn!("failed to write table store: {e}");
    }
}

fn save_store(runtime: NonSendMut<ScriptRuntime>) {
    let dirty = runtime.host.borrow().store_dirty;
    if dirty {
        write_store(&runtime);
        runtime.host.borrow_mut().store_dirty = false;
    }
}

/// Canonical key codes passed to the script (see prelude constants).
fn key_code(key: KeyCode) -> Option<i64> {
    Some(match key {
        KeyCode::ShiftLeft | KeyCode::ArrowLeft => 1,
        KeyCode::ShiftRight | KeyCode::ArrowRight => 2,
        KeyCode::Enter => 3,
        KeyCode::Digit1 => 4,
        KeyCode::Digit5 => 5,
        KeyCode::KeyZ => 6,
        KeyCode::Slash => 7,
        KeyCode::Space => 8,
        _ => return None,
    })
}

/// Captures a ball entering a kicker sensor: frozen kinematic at the kicker
/// centre until the script kicks or destroys it, vpinball's locked-in-kicker
/// behaviour. Balls escaping a kicker they were just kicked out of (or created
/// in) are left alone until they exit its sensor.
fn capture_balls(
    mut commands: Commands,
    mut started: MessageReader<CollisionStart>,
    kickers: Query<&Transform, With<Kicker>>,
    mut balls: Query<
        (
            &mut Transform,
            &mut LinearVelocity,
            Option<&KickerEscape>,
            Option<&CapturedBall>,
        ),
        (With<Ball>, Without<Kicker>),
    >,
) {
    for collision in started.read() {
        let (ball_entity, kicker_entity) =
            if balls.contains(collision.collider1) && kickers.contains(collision.collider2) {
                (collision.collider1, collision.collider2)
            } else if balls.contains(collision.collider2) && kickers.contains(collision.collider1) {
                (collision.collider2, collision.collider1)
            } else {
                continue;
            };
        let Ok((mut transform, mut velocity, escape, captured)) = balls.get_mut(ball_entity) else {
            continue;
        };
        if captured.is_some() || escape.is_some_and(|e| e.kicker == kicker_entity) {
            continue;
        }
        let kicker_pos = kickers.get(kicker_entity).unwrap().translation.truncate();
        transform.translation.x = kicker_pos.x;
        transform.translation.y = kicker_pos.y;
        velocity.0 = Vec2::ZERO;
        commands.entity(ball_entity).insert((
            CapturedBall {
                kicker: kicker_entity,
            },
            RigidBody::Kinematic,
        ));
    }
}

/// Drops the escape marker once the kicked ball has actually left the kicker.
fn clear_kicker_escapes(
    mut commands: Commands,
    mut ended: MessageReader<CollisionEnd>,
    balls: Query<&KickerEscape, With<Ball>>,
) {
    for collision in ended.read() {
        for (ball, other) in [
            (collision.collider1, collision.collider2),
            (collision.collider2, collision.collider1),
        ] {
            if let Ok(escape) = balls.get(ball)
                && escape.kicker == other
            {
                commands.entity(ball).remove::<KickerEscape>();
            }
        }
    }
}

/// Forwards key transitions to the script. Diffs the `pressed` state against the
/// previous frame instead of reading `just_pressed`: the remote-control interface
/// injects presses mid-`Update` (after this system), which `just_pressed` would
/// miss once the input system clears it the next frame.
fn forward_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut previous: Local<Vec<KeyCode>>,
    mut runtime: NonSendMut<ScriptRuntime>,
) {
    let now: Vec<KeyCode> = keyboard
        .get_pressed()
        .copied()
        .filter(|key| key_code(*key).is_some())
        .collect();
    for key in &now {
        if !previous.contains(key)
            && let Some(code) = key_code(*key)
        {
            runtime.dispatch("table_keydown", &[ScriptValue::Int(code)]);
        }
    }
    for key in previous.iter() {
        if !now.contains(key)
            && let Some(code) = key_code(*key)
        {
            runtime.dispatch("table_keyup", &[ScriptValue::Int(code)]);
        }
    }
    *previous = now;
}

fn forward_collisions(
    mut started: MessageReader<CollisionStart>,
    mut ended: MessageReader<CollisionEnd>,
    balls: Query<(), With<Ball>>,
    names: Query<&ScriptName>,
    mut runtime: NonSendMut<ScriptRuntime>,
) {
    let item_of = |a: Entity, b: Entity| -> Option<String> {
        let (_, item) = if balls.contains(a) {
            (a, b)
        } else if balls.contains(b) {
            (b, a)
        } else {
            return None;
        };
        names.get(item).ok().map(|n| n.0.to_lowercase())
    };
    for collision in started.read() {
        if let Some(name) = item_of(collision.collider1, collision.collider2) {
            runtime.dispatch(&format!("{name}_hit"), &[]);
        }
    }
    for collision in ended.read() {
        if let Some(name) = item_of(collision.collider1, collision.collider2) {
            runtime.dispatch(&format!("{name}_unhit"), &[]);
        }
    }
}

fn forward_slingshots(
    mut fired: MessageReader<SlingshotFired>,
    mut runtime: NonSendMut<ScriptRuntime>,
) {
    for sling in fired.read() {
        runtime.dispatch(&format!("{}_slingshot", sling.name.to_lowercase()), &[]);
    }
}

fn forward_spins(mut spun: MessageReader<SpinnerSpun>, mut runtime: NonSendMut<ScriptRuntime>) {
    for spin in spun.read() {
        runtime.dispatch(&format!("{}_spin", spin.name.to_lowercase()), &[]);
    }
}

fn tick_timers(time: Res<Time>, mut runtime: NonSendMut<ScriptRuntime>) {
    let dt_ms = time.delta_secs_f64() * 1000.0;
    // Collect first: dispatch needs &mut runtime while timers are part of it.
    let mut due: Vec<String> = Vec::new();
    for timer in &mut runtime.timers {
        if !timer.enabled {
            continue;
        }
        if timer.interval_ms <= 0.0 {
            // vpinball interval -1: fire every frame.
            due.push(timer.lower.clone());
            continue;
        }
        timer.elapsed_ms += dt_ms;
        // Fire at most once per frame: timer scripts (reel steps, chimes) want
        // pacing, not catch-up bursts after a hitch.
        if timer.elapsed_ms >= timer.interval_ms {
            timer.elapsed_ms %= timer.interval_ms;
            due.push(timer.lower.clone());
        }
    }
    for name in due {
        runtime.dispatch(&format!("{name}_timer"), &[]);
    }
}

/// Applies the side effects the script queued: light states, timer switches,
/// sounds, kicker ball operations and the flipper gate. Reel and textbox
/// writes only update the shadow state, which the scoreboard displays.
fn apply_commands(
    mut commands: Commands,
    mut runtime: NonSendMut<ScriptRuntime>,
    mut lights: Query<(&Light, Option<&mut LightAnimation>, &mut Visibility)>,
    kickers: Query<(Entity, &Kicker, &Transform)>,
    mut balls: Query<
        (
            Entity,
            &Transform,
            &mut LinearVelocity,
            Option<&CapturedBall>,
        ),
        With<Ball>,
    >,
    sounds: Res<ScriptSounds>,
    playing: Query<(Entity, &PlayingSound)>,
    mut flippers_enabled: ResMut<FlippersEnabled>,
    table_assets: Option<Res<TableAssets>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut ball_materials: ResMut<Assets<BallMaterial>>,
    ball_assets: Option<Res<BallAssets>>,
    assets_vpx: Res<Assets<VpxAsset>>,
) {
    // Retry kicks whose ball had not spawned yet.
    let mut pending = std::mem::take(&mut runtime.pending_kicks);
    pending.retain_mut(|kick| {
        if try_kick(
            &mut commands,
            &kickers,
            &mut balls,
            &kick.name,
            kick.angle,
            kick.speed,
        ) {
            return false;
        }
        kick.tries -= 1;
        if kick.tries == 0 {
            warn!("kicker {} kick found no ball to kick", kick.name);
        }
        kick.tries > 0
    });
    runtime.pending_kicks = pending;

    let queued: Vec<ScriptCommand> = std::mem::take(&mut runtime.host.borrow_mut().commands);
    for command in queued {
        match command {
            ScriptCommand::SetProp { name, prop, value } => {
                let kind = runtime
                    .host
                    .borrow()
                    .item(&name)
                    .map(|item| item.kind)
                    .unwrap_or(ItemKind::Other);
                match (kind, prop.as_str()) {
                    (ItemKind::Light, "state") => {
                        let state = value.as_f32().unwrap_or(0.0);
                        for (light, animation, mut visibility) in &mut lights {
                            if !light.name.eq_ignore_ascii_case(&name) {
                                continue;
                            }
                            match animation {
                                Some(mut animation) => animation.set_state(state),
                                // GI lights have no animation; toggle them.
                                None => {
                                    *visibility = if state != 0.0 {
                                        Visibility::Inherited
                                    } else {
                                        Visibility::Hidden
                                    };
                                }
                            }
                        }
                    }
                    (ItemKind::Timer, "enabled") => {
                        let enabled = value.as_bool().unwrap_or(false);
                        let lower = name.to_lowercase();
                        for timer in &mut runtime.timers {
                            if timer.lower == lower {
                                timer.enabled = enabled;
                                // Enabling restarts the countdown, like vpinball.
                                timer.elapsed_ms = 0.0;
                            }
                        }
                    }
                    (ItemKind::Timer, "interval") => {
                        let interval = value.as_f32().unwrap_or(100.0) as f64;
                        let lower = name.to_lowercase();
                        for timer in &mut runtime.timers {
                            if timer.lower == lower {
                                timer.interval_ms = interval;
                            }
                        }
                    }
                    // Textbox/reel text lives in the shadow state only; the
                    // scoreboard renders it.
                    (ItemKind::TextBox, _) | (ItemKind::Reel, _) => {}
                    _ => {
                        debug!("script set unhandled {name}.{prop} = {value:?}");
                    }
                }
            }
            ScriptCommand::Call { name, method, args } => {
                let kind = runtime
                    .host
                    .borrow()
                    .item(&name)
                    .map(|item| item.kind)
                    .unwrap_or(ItemKind::Other);
                match (kind, method.as_str()) {
                    (ItemKind::Kicker, "createball") => {
                        let Some((_, _, transform)) = kickers
                            .iter()
                            .find(|(_, k, _)| k.name.eq_ignore_ascii_case(&name))
                        else {
                            continue;
                        };
                        let (Some(table_assets), Some(ball_assets)) =
                            (table_assets.as_ref(), ball_assets.as_ref())
                        else {
                            continue;
                        };
                        commands.spawn((
                            ball_bundle(
                                0,
                                table_assets,
                                &mut meshes,
                                &mut ball_materials,
                                ball_assets,
                                &assets_vpx,
                                transform.translation.truncate(),
                            ),
                            DespawnOnExit(Screen::Gameplay),
                        ));
                    }
                    (ItemKind::Kicker, "kick") => {
                        let angle = args.first().and_then(|v| v.as_f32()).unwrap_or(0.0);
                        let speed = args.get(1).and_then(|v| v.as_f32()).unwrap_or(0.0);
                        if !try_kick(&mut commands, &kickers, &mut balls, &name, angle, speed) {
                            // The ball may have been created this very frame
                            // (spawns are deferred); retry for a while.
                            runtime.pending_kicks.push(PendingKick {
                                name,
                                angle,
                                speed,
                                tries: 60,
                            });
                        }
                    }
                    (ItemKind::Kicker, "destroyball") => {
                        let Some((kicker_entity, _, kicker_transform)) = kickers
                            .iter()
                            .find(|(_, k, _)| k.name.eq_ignore_ascii_case(&name))
                        else {
                            continue;
                        };
                        let kicker_pos = kicker_transform.translation.truncate();
                        // Prefer the ball captured by this kicker, else nearest.
                        if let Some((entity, _, _, _)) = balls
                            .iter_mut()
                            .map(|(e, t, v, c)| {
                                let captured = c.is_some_and(|c| c.kicker == kicker_entity);
                                (
                                    e,
                                    t.translation.truncate().distance(kicker_pos),
                                    v,
                                    captured,
                                )
                            })
                            .filter(|(_, d, _, captured)| *captured || *d < 0.05)
                            .min_by(|a, b| a.1.total_cmp(&b.1))
                        {
                            commands.entity(entity).despawn();
                        }
                    }
                    (ItemKind::Reel, "setvalue") => {
                        set_reel(&runtime, &name, args.first().and_then(|v| v.as_i64()));
                    }
                    (ItemKind::Reel, "addvalue") => {
                        let current = runtime
                            .host
                            .borrow()
                            .get_prop(&name, "value")
                            .as_i64()
                            .unwrap_or(0);
                        let delta = args.first().and_then(|v| v.as_i64()).unwrap_or(0);
                        set_reel(&runtime, &name, Some(current + delta));
                    }
                    (ItemKind::Reel, "resettozero") => {
                        set_reel(&runtime, &name, Some(0));
                    }
                    _ => {
                        debug!("script called unhandled {name}:{method}");
                    }
                }
            }
            ScriptCommand::PlaySound { name } => {
                if let Some(handle) = sounds.0.get(&name.to_lowercase()) {
                    commands.spawn((
                        AudioPlayer(handle.clone()),
                        PlaybackSettings::DESPAWN,
                        PlayingSound(name.to_lowercase()),
                    ));
                } else {
                    debug!("script played unknown sound '{name}'");
                }
            }
            ScriptCommand::StopSound { name } => {
                let lower = name.to_lowercase();
                for (entity, sound) in &playing {
                    if sound.0 == lower {
                        commands.entity(entity).despawn();
                    }
                }
            }
            ScriptCommand::SetFlippersEnabled(enabled) => {
                flippers_enabled.0 = enabled;
            }
        }
    }
}

/// Kick the ball sitting in the named kicker: release a captured ball back to
/// dynamic and throw it at the vpinball angle/speed (degrees, VP speed units;
/// angle 0 is up the table, 90 to the right). Returns whether a ball was hit.
fn try_kick(
    commands: &mut Commands,
    kickers: &Query<(Entity, &Kicker, &Transform)>,
    balls: &mut Query<
        (
            Entity,
            &Transform,
            &mut LinearVelocity,
            Option<&CapturedBall>,
        ),
        With<Ball>,
    >,
    name: &str,
    angle: f32,
    speed: f32,
) -> bool {
    let Some((kicker_entity, _, kicker_transform)) = kickers
        .iter()
        .find(|(_, k, _)| k.name.eq_ignore_ascii_case(name))
    else {
        return true; // unknown kicker: drop the kick, nothing to retry
    };
    let kicker_pos = kicker_transform.translation.truncate();
    // vpinball speed units: 18.53 per m/s (see pinball::physics).
    let speed_m = speed / 18.53;
    let direction = Vec2::new(angle.to_radians().sin(), angle.to_radians().cos());
    let Some((entity, _, mut velocity, _)) = balls
        .iter_mut()
        .map(|(e, t, v, c)| {
            let captured = c.is_some_and(|c| c.kicker == kicker_entity);
            (
                e,
                t.translation.truncate().distance(kicker_pos),
                v,
                captured,
            )
        })
        .filter(|(_, d, _, captured)| *captured || *d < 0.05)
        .min_by(|a, b| a.1.total_cmp(&b.1))
    else {
        return false;
    };
    velocity.0 = direction * speed_m;
    commands.entity(entity).remove::<CapturedBall>().insert((
        RigidBody::Dynamic,
        KickerEscape {
            kicker: kicker_entity,
        },
    ));
    true
}

/// Write a reel's value into the shadow state (the scoreboard reads it).
fn set_reel(runtime: &ScriptRuntime, name: &str, value: Option<i64>) {
    if let Some(value) = value {
        let mut host = runtime.host.borrow_mut();
        let key = name.to_lowercase();
        if let Some(item) = host.items.get_mut(&key) {
            item.props.insert("value".into(), ScriptValue::Int(value));
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::api::*;
    use super::lua::LuaEngine;
    use bevy::platform::collections::HashMap;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn test_host() -> SharedHost {
        let mut host = HostState::default();
        for (name, kind) in [
            ("ShootAgain", ItemKind::Light),
            ("CredTimer", ItemKind::Timer),
            ("Reel1", ItemKind::Reel),
            ("nb", ItemKind::Kicker),
        ] {
            let mut item = ItemState {
                kind,
                name: name.to_string(),
                props: HashMap::default(),
            };
            if kind == ItemKind::Light {
                item.props.insert("state".into(), ScriptValue::Num(0.0));
            }
            host.items.insert(name.to_lowercase(), item);
        }
        Rc::new(RefCell::new(host))
    }

    /// The Lua prelude resolves bare item names case-insensitively, routes
    /// property writes through the shadow state and queues methods/host calls.
    #[test]
    fn lua_bridge_round_trip() {
        let host = test_host();
        let collections = vec![("GI".to_string(), vec!["ShootAgain".to_string()])];
        let mut engine = LuaEngine::new(host.clone(), &collections).unwrap();
        engine
            .load(
                r#"
                function table_init()
                    shootagain.state = LightStateOn
                    assert(SHOOTAGAIN.state == 1)
                    credtimer.enabled = true
                    nb:kick(135, 4)
                    playsound("click")
                    for _, l in ipairs(GI) do l.state = LightStateBlinking end
                end
                "#,
            )
            .unwrap();
        let handled = engine.dispatch("table_init", &[]).unwrap();
        assert!(handled);
        assert!(!engine.dispatch("no_such_handler", &[]).unwrap());

        let host = host.borrow();
        assert_eq!(
            host.get_prop("shootagain", "state").as_f32(),
            Some(2.0),
            "collection write lands on the member"
        );
        assert_eq!(
            host.get_prop("credtimer", "enabled"),
            ScriptValue::Bool(true)
        );
        let kicks: Vec<_> = host
            .commands
            .iter()
            .filter(|c| matches!(c, ScriptCommand::Call { method, .. } if method == "kick"))
            .collect();
        assert_eq!(kicks.len(), 1);
        assert!(
            host.commands
                .iter()
                .any(|c| matches!(c, ScriptCommand::PlaySound { name } if name == "click"))
        );
    }
}

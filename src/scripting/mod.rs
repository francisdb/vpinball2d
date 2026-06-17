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
mod flexdmd;
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
use avian2d::prelude::{ColliderDisabled, CollisionEnd, CollisionStart, LinearVelocity, RigidBody};
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

/// A credit reel (see `sidecar`): a single-window
/// [`ScoreReel`](crate::pinball::reel::ScoreReel) driven by the
/// numeric text of a script textbox, so the credit count rolls like a B2S
/// credit window. The script only sets the textbox; the rolling is the engine's.
#[derive(Component)]
struct CreditReel {
    /// The textbox whose value drives the reel.
    textbox: String,
    /// Last value pushed to the reel, to roll only the delta.
    last_value: i64,
}

/// A ball held in a kicker (saucer or drain), frozen at its centre until the
/// script kicks or destroys it - vpinball's locked-in-kicker state.
#[derive(Component)]
pub struct CapturedBall {
    kicker: Entity,
}

/// The kicker whose hit-circle a ball is currently *inside*, vpinball's per-ball
/// volume set (`m_vpVolObjs`) reduced to the single membership a 2D ball can have
/// at once. Capture only fires on a fresh entry (no membership for that kicker);
/// a kicked ball keeps its membership until it geometrically leaves the circle,
/// so it cannot be re-grabbed until it leaves and returns - no collision events,
/// no timers. (A ball realistically overlaps at most one kicker, so one slot.)
#[derive(Component)]
struct InKickerVolume {
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

    /// The shared host state, for systems that read script-owned state (e.g. the
    /// FlexDMD renderer reads the scene graph the script built).
    pub(crate) fn host(&self) -> SharedHost {
        self.host.clone()
    }
}

pub fn plugin(app: &mut App) {
    app.add_message::<SlingshotFired>();
    app.add_message::<SpinnerSpun>();
    app.add_systems(
        Update,
        (
            capture_balls,
            forward_keys,
            forward_collisions,
            forward_slingshots,
            forward_spins,
            tick_timers,
            apply_commands,
            sync_credit_reel,
            save_store,
            scoreboard::sync_desktop_texts,
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
/// sidecar; used by the table picker. A table counts as scripted if it has a
/// `.lua` (game logic) and/or a `.table.json` (static sound/animation config)
/// next to the vpx.
pub fn has_script_sidecar(tables_dir: &std::path::Path, rel_path: &str) -> bool {
    let base = tables_dir.join(rel_path);
    base.with_extension("lua").is_file() || base.with_extension("table.json").is_file()
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
            GameItemEnum::HitTarget(target) => {
                state.kind = ItemKind::Target;
                state
                    .props
                    .insert("isdropped".into(), ScriptValue::Bool(target.is_dropped));
            }
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
            GameItemEnum::Flasher(flasher) => {
                state.kind = ItemKind::Flasher;
                state
                    .props
                    .insert("imagea".into(), ScriptValue::Str(flasher.image_a.clone()));
                state
                    .props
                    .insert("visible".into(), ScriptValue::Bool(flasher.is_visible));
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

/// vpinball's kicker capture by geometry (`KickerHitCircle::DoCollide`), driven
/// by distance rather than collision events so the kinematic/dynamic swap on
/// kick cannot churn it. Each frame, for every free (uncaptured) ball:
///
/// - inside a kicker's hit circle for the *first* time (no [`InKickerVolume`]
///   membership for it) -> capture: freeze kinematic at the centre, record the
///   membership;
/// - inside the kicker it is already a member of (e.g. just kicked, still
///   overlapping) -> leave it; it cannot be re-grabbed until it leaves;
/// - outside every kicker -> drop any stale membership, so a later return is a
///   fresh entry.
///
/// A captured ball moved off its kicker by something other than a kick (mouse
/// ball control, the remote teleport) is released, so it cannot stay kinematic
/// and float through the table.
fn capture_balls(
    mut commands: Commands,
    kickers: Query<(Entity, &Kicker, &Transform)>,
    mut balls: Query<
        (
            Entity,
            &mut Transform,
            &mut LinearVelocity,
            Option<&InKickerVolume>,
            Option<&CapturedBall>,
        ),
        (With<Ball>, Without<Kicker>),
    >,
) {
    for (ball, mut transform, mut velocity, membership, captured) in &mut balls {
        let ball_pos = transform.translation.truncate();

        // A held ball stays put until the script kicks it - unless something
        // dragged it off its kicker, in which case release it (it would float
        // through everything while kinematic).
        if let Some(captured) = captured {
            let off = match kickers.get(captured.kicker) {
                Ok((_, kicker, kt)) => ball_pos.distance(kt.translation.truncate()) > kicker.radius,
                Err(_) => true, // kicker gone; nothing will kick it free
            };
            if off {
                commands
                    .entity(ball)
                    .remove::<CapturedBall>()
                    .insert(RigidBody::Dynamic);
            }
            continue;
        }

        // Nearest kicker whose hit circle this ball's centre is inside.
        let inside = kickers
            .iter()
            .map(|(e, k, t)| {
                (
                    e,
                    t.translation.truncate(),
                    ball_pos.distance(t.translation.truncate()),
                    k.radius,
                )
            })
            .filter(|(_, _, d, radius)| *d <= *radius)
            .min_by(|a, b| a.2.total_cmp(&b.2));

        let Some((kicker_entity, center, _, _)) = inside else {
            // Left every kicker: clear stale membership so a return re-captures.
            if membership.is_some() {
                commands.entity(ball).remove::<InKickerVolume>();
            }
            continue;
        };

        // Already a member of this kicker's volume (just kicked, or mid-overlap):
        // vpinball will not re-grab until the ball leaves and returns.
        if membership.is_some_and(|m| m.kicker == kicker_entity) {
            continue;
        }

        // Fresh entry into the hit circle: capture.
        transform.translation.x = center.x;
        transform.translation.y = center.y;
        velocity.0 = Vec2::ZERO;
        commands.entity(ball).insert((
            CapturedBall {
                kicker: kicker_entity,
            },
            InKickerVolume {
                kicker: kicker_entity,
            },
            RigidBody::Kinematic,
        ));
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

/// The table's vpx data (bundled to keep `apply_commands` under Bevy's system
/// parameter limit).
#[derive(bevy::ecs::system::SystemParam)]
struct VpxAssets<'w> {
    table: Option<Res<'w, TableAssets>>,
    vpx: Res<'w, Assets<VpxAsset>>,
}

impl VpxAssets<'_> {
    fn asset(&self) -> Option<&VpxAsset> {
        self.table.as_ref().and_then(|t| self.vpx.get(&t.vpx))
    }
}

/// Droppables addressed by `<name>.IsDropped`: drop-target gameitems and droppable walls
/// (drop targets / flipper-gap posts). Bundled so `apply_commands` stays under Bevy's param limit.
#[derive(bevy::ecs::system::SystemParam)]
struct DroppableIo<'w, 's> {
    walls: Query<'w, 's, (Entity, &'static ScriptName), With<crate::pinball::wall::Droppable>>,
    targets: Query<
        'w,
        's,
        (
            &'static ScriptName,
            &'static mut crate::pinball::targets::DropTarget,
        ),
    >,
}

/// Runtime flasher canvases plus their materials, for `flasher.ImageA = ...`.
#[derive(bevy::ecs::system::SystemParam)]
struct FlasherIo<'w, 's> {
    materials: ResMut<'w, Assets<ColorMaterial>>,
    query: Query<
        'w,
        's,
        (
            Entity,
            &'static ScriptName,
            &'static MeshMaterial2d<ColorMaterial>,
            &'static crate::pinball::flasher::FlasherCanvas,
        ),
    >,
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
    vpx: VpxAssets,
    mut meshes: ResMut<Assets<Mesh>>,
    mut ball_materials: ResMut<Assets<BallMaterial>>,
    ball_assets: Option<Res<BallAssets>>,
    mut reels: Query<&mut crate::pinball::reel::ScoreReel>,
    mut droppables: DroppableIo,
    mut flipper_query: Query<(Entity, &mut crate::pinball::flipper::Flipper)>,
    mut flasher_io: FlasherIo,
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
                    // A droppable wall (drop target / flipper-gap post): dropped
                    // disables its collider and hides it; raised restores both.
                    (ItemKind::Wall, "isdropped") => {
                        let dropped = value.as_bool().unwrap_or(false);
                        for (entity, sname) in &droppables.walls {
                            if !sname.0.eq_ignore_ascii_case(&name) {
                                continue;
                            }
                            if dropped {
                                commands
                                    .entity(entity)
                                    .insert((ColliderDisabled, Visibility::Hidden));
                            } else {
                                commands
                                    .entity(entity)
                                    .remove::<ColliderDisabled>()
                                    .insert(Visibility::Inherited);
                            }
                        }
                    }
                    // A drop target: the rules drive it via `IsDropped`. Flip the flag; the
                    // pinball::targets system fades it and toggles its collider.
                    (ItemKind::Target, "isdropped") => {
                        let dropped = value.as_bool().unwrap_or(false);
                        for (sname, mut drop_target) in &mut droppables.targets {
                            if sname.0.eq_ignore_ascii_case(&name) {
                                drop_target.dropped = dropped;
                            }
                        }
                    }
                    // A flipper enable/disable (sliding "gap" tables swap the two
                    // flippers per side): toggle the live flag, collider and visual.
                    (ItemKind::Flipper, "enabled") => {
                        let on = value.as_bool().unwrap_or(true);
                        for (entity, mut flipper) in &mut flipper_query {
                            if !flipper.name.eq_ignore_ascii_case(&name) {
                                continue;
                            }
                            flipper.enabled = on;
                            if on {
                                commands
                                    .entity(entity)
                                    .remove::<ColliderDisabled>()
                                    .insert(Visibility::Inherited);
                            } else {
                                commands
                                    .entity(entity)
                                    .insert((ColliderDisabled, Visibility::Hidden));
                            }
                        }
                    }
                    // A flasher used as a runtime canvas (e.g. a reel/grid DMD's
                    // digit cells): swap its texture to the named vpx image.
                    (ItemKind::Flasher, "imagea") => {
                        let img_name = match &value {
                            ScriptValue::Str(s) => s.clone(),
                            _ => String::new(),
                        };
                        let handle = vpx.asset().and_then(|v| v.image(&img_name).cloned());
                        for (entity, sname, mat, canvas) in &flasher_io.query {
                            if !sname.0.eq_ignore_ascii_case(&name) {
                                continue;
                            }
                            if let Some(material) = flasher_io.materials.get_mut(&mat.0) {
                                material.texture = handle.clone();
                                material.color.set_alpha(if handle.is_some() {
                                    canvas.alpha
                                } else {
                                    0.0
                                });
                            }
                            commands.entity(entity).insert(if handle.is_some() {
                                Visibility::Inherited
                            } else {
                                Visibility::Hidden
                            });
                        }
                    }
                    (ItemKind::Flasher, "visible") => {
                        let vis = value.as_bool().unwrap_or(true);
                        for (entity, sname, _, _) in &flasher_io.query {
                            if sname.0.eq_ignore_ascii_case(&name) {
                                commands.entity(entity).insert(if vis {
                                    Visibility::Inherited
                                } else {
                                    Visibility::Hidden
                                });
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
                            (vpx.table.as_ref(), ball_assets.as_ref())
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
                                &vpx.vpx,
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
                        if let Some(mut reel) = score_reel(&mut reels, &name) {
                            reel.set_value(args.first().and_then(|v| v.as_i64()).unwrap_or(0));
                        }
                    }
                    (ItemKind::Reel, "addvalue") => {
                        if let Some(mut reel) = score_reel(&mut reels, &name) {
                            reel.add_value(args.first().and_then(|v| v.as_i64()).unwrap_or(0));
                        }
                    }
                    (ItemKind::Reel, "resettozero") => {
                        if let Some(mut reel) = score_reel(&mut reels, &name) {
                            reel.reset_to_zero();
                        }
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
    // Release the lock but keep (or set) the volume membership: the kicked ball
    // is still inside the hit circle, so vpinball does not re-grab it until it
    // leaves and returns. Setting it also covers a ball just created in the
    // kicker (createball) and kicked the same frame.
    commands.entity(entity).remove::<CapturedBall>().insert((
        RigidBody::Dynamic,
        InKickerVolume {
            kicker: kicker_entity,
        },
    ));
    true
}

/// Rolls each credit reel to its source textbox's value: parse the textbox's
/// numeric text, clamp to the strip's range, and roll the delta. The script
/// only ever sets the textbox text; the roll is the engine's.
fn sync_credit_reel(
    runtime: NonSend<ScriptRuntime>,
    mut reels: Query<(&mut crate::pinball::reel::ScoreReel, &mut CreditReel)>,
) {
    if reels.is_empty() {
        return;
    }
    let host = runtime.host.borrow();
    for (mut reel, mut credit) in &mut reels {
        let ScriptValue::Str(text) = host.get_prop(&credit.textbox, "text") else {
            continue;
        };
        let Ok(value) = text.trim().parse::<i64>() else {
            continue;
        };
        // The credit strip tops out at its highest cell.
        let target = value.clamp(0, reel.max_value());
        if target != credit.last_value {
            if credit.last_value == i64::MIN {
                reel.set_value(target);
            } else {
                reel.add_value(target - credit.last_value);
            }
            credit.last_value = target;
        }
    }
}

/// The animated reel entity for a script reel name (case-insensitive).
fn score_reel<'a>(
    reels: &'a mut Query<&mut crate::pinball::reel::ScoreReel>,
    name: &str,
) -> Option<Mut<'a, crate::pinball::reel::ScoreReel>> {
    reels
        .iter_mut()
        .find(|reel| reel.name.eq_ignore_ascii_case(name))
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

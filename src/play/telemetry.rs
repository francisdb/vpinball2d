//! Telemetry: write the game state as JSON to [`STATE_JSON_PATH`] (a ~50 Hz overwrite snapshot)
//! and [`STATE_JSONL_PATH`] (an append-only stream), and append a line per ball/object contact to
//! [`EVENTS_PATH`]. Read-only observation; enabled by the `telemetry` feature.

use crate::pinball::ball::Ball;
use crate::pinball::flipper::Flipper;
use crate::pinball::plunger::Plunger;
use crate::pinball::table::TableAssets;
use crate::pinball::wall::Slingshot;
use crate::screens::Screen;
use crate::vpx::VpxAsset;
use avian2d::prelude::*;
use bevy::prelude::*;
use std::fmt::Write as _;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write as _;
use vpin::vpx::units::vpu_to_m;

/// Telemetry file path prefix; override with `VPINBALL_TELEMETRY` so several
/// instances (e.g. parallel headless tests) write to separate files.
static PREFIX: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    std::env::var("VPINBALL_TELEMETRY").unwrap_or_else(|_| "/tmp/vpinball2d".to_string())
});
/// File overwritten with the latest telemetry frame as one JSON object (machine-readable).
static STATE_JSON_PATH: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| format!("{}_state.json", &*PREFIX));
/// File appended with one JSON object per telemetry frame: a stream a reader can tail to never
/// miss a frame between overwrite snapshots. Truncated once when gameplay starts.
static STATE_JSONL_PATH: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| format!("{}_state.jsonl", &*PREFIX));
/// File appended with one line per ball/object contact (a running event log).
static EVENTS_PATH: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| format!("{}_events.log", &*PREFIX));

pub(super) fn plugin(app: &mut App) {
    info!(
        "Telemetry enabled: state={} events={}",
        &*STATE_JSON_PATH, &*EVENTS_PATH
    );
    app.insert_resource(TelemetryTimer(Timer::from_seconds(
        0.02,
        TimerMode::Repeating,
    )));
    // Start each gameplay session with a fresh event log and telemetry stream.
    app.add_systems(OnEnter(Screen::Gameplay), |_: Commands| {
        let _ = fs::write(&*EVENTS_PATH, "");
        let _ = fs::write(&*STATE_JSONL_PATH, "");
    });
    app.add_systems(
        Update,
        (write_telemetry, log_contacts).run_if(in_state(Screen::Gameplay)),
    );
}

#[derive(Resource)]
struct TelemetryTimer(Timer);

/// Quote and escape a string as a JSON string literal (enough for object names).
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Append one line per new ball/object contact to [`EVENTS_PATH`]. Reads the same
/// `CollisionStart` stream the gameplay systems use; names the struck object by its `Name`
/// (falling back to `<unnamed>`), so the operator can see what the ball hits in real time.
fn log_contacts(
    mut collisions: MessageReader<CollisionStart>,
    time: Res<Time>,
    balls: Query<&Ball>,
    names: Query<&Name>,
) {
    let mut buf = String::new();
    for collision in collisions.read() {
        let (e1, e2) = (collision.collider1, collision.collider2);
        // Only log contacts that involve a ball; name the other side.
        let (ball, other) = if let Ok(b) = balls.get(e1) {
            (b, e2)
        } else if let Ok(b) = balls.get(e2) {
            (b, e1)
        } else {
            continue;
        };
        let other_name = names
            .get(other)
            .map(|n| n.as_str().to_owned())
            .unwrap_or_else(|_| "<unnamed>".to_owned());
        let _ = writeln!(
            buf,
            "{:8.3}  ball {} hit {}",
            time.elapsed_secs(),
            ball.id,
            other_name
        );
    }
    if !buf.is_empty()
        && let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&*EVENTS_PATH)
    {
        let _ = f.write_all(buf.as_bytes());
    }
}

/// Write the telemetry frame (playfield bounds, static objects and per-ball state) as one JSON
/// object snapshot, and append it to the JSON stream. All coordinates are world metres (the table
/// is centred on the origin, see pinball::table), so the operator can aim a ball straight at it.
fn write_telemetry(
    time: Res<Time>,
    mut timer: ResMut<TelemetryTimer>,
    table_assets: Option<Res<TableAssets>>,
    assets_vpx: Res<Assets<VpxAsset>>,
    balls: Query<(&Ball, &Transform, &LinearVelocity)>,
    // Bumpers and kickers are classified by their Name prefix; flippers and the plunger are
    // read through their components so we can also report their up/down (engaged) state.
    named: Query<(&Name, &Transform), Without<Ball>>,
    flippers: Query<(&Flipper, &Transform, &Name)>,
    plungers: Query<(&Plunger, &Transform, &Name)>,
    slingshots: Query<&Slingshot>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    let Some(table_assets) = table_assets.as_ref() else {
        return;
    };
    let Some(vpx) = assets_vpx.get(&table_assets.vpx) else {
        return;
    };
    // Playfield bounds in world metres: centred on the origin (see pinball::table).
    let half_w = vpu_to_m(vpx.raw.gamedata.right - vpx.raw.gamedata.left) / 2.0;
    let half_h = vpu_to_m(vpx.raw.gamedata.bottom - vpx.raw.gamedata.top) / 2.0;
    if half_w <= 0.0 || half_h <= 0.0 {
        return;
    }

    // Collect each object as a JSON fragment for the machine-readable snapshot and stream.
    let mut j_bumpers: Vec<String> = Vec::new();
    let mut j_kickers: Vec<String> = Vec::new();
    let mut j_flippers: Vec<String> = Vec::new();
    let mut j_plungers: Vec<String> = Vec::new();
    let mut j_slingshots: Vec<String> = Vec::new();
    let mut j_balls: Vec<String> = Vec::new();

    // Static objects we can aim at: bumpers and kickers, by Name prefix.
    for (name, t) in &named {
        let n = name.as_str();
        let kind = if n.starts_with("Bumper") {
            "bumper"
        } else if n.starts_with("Kicker ") {
            "kicker"
        } else {
            continue;
        };
        let p = t.translation.truncate();
        let obj = format!(
            "{{\"name\":{},\"pos\":[{:.3},{:.3}]}}",
            json_str(n),
            p.x,
            p.y
        );
        if kind == "bumper" {
            j_bumpers.push(obj);
        } else {
            j_kickers.push(obj);
        }
    }

    // Flippers: report position plus `raised` (0 resting .. 1 fully energised, from the body
    // angle between rest and active) and `pressed` (the button latch), so a controller can
    // confirm a hold/tap engaged instead of inferring it from the ball.
    for (flipper, t, name) in &flippers {
        let p = t.translation.truncate();
        let angle = t.rotation.to_euler(EulerRot::ZYX).0;
        let span = flipper.active_angle - flipper.rest_angle;
        let raised = if span.abs() > 1e-6 {
            ((angle - flipper.rest_angle) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        j_flippers.push(format!(
            "{{\"name\":{},\"pos\":[{:.3},{:.3}],\"raised\":{:.3},\"pressed\":{}}}",
            json_str(name.as_str()),
            p.x,
            p.y,
            raised,
            flipper.pressed
        ));
    }

    // Plunger: report position plus `pulled` (0 resting .. 1 fully drawn back).
    for (plunger, t, name) in &plungers {
        let p = t.translation.truncate();
        let pulled = plunger.pulled();
        j_plungers.push(format!(
            "{{\"name\":{},\"pos\":[{:.3},{:.3}],\"pulled\":{:.3}}}",
            json_str(name.as_str()),
            p.x,
            p.y,
            pulled
        ));
    }

    // Slingshots: world centre (to aim at) plus the firing `force`/`threshold` being calibrated.
    for slingshot in &slingshots {
        j_slingshots.push(format!(
            "{{\"name\":{},\"pos\":[{:.3},{:.3}],\"force\":{:.4},\"threshold\":{:.4}}}",
            json_str(&slingshot.name),
            slingshot.center.x,
            slingshot.center.y,
            slingshot.force,
            slingshot.threshold
        ));
    }

    // Report every ball by its real id (not iteration index), so a stray auto-released table
    // ball is never confused with another. Sorted by id.
    let mut ball_rows: Vec<(u32, Vec2, Vec2)> = balls
        .iter()
        .map(|(b, t, v)| (b.id, t.translation.truncate(), v.0))
        .collect();
    ball_rows.sort_by_key(|(id, _, _)| *id);
    for (id, p, v) in &ball_rows {
        j_balls.push(format!(
            "{{\"id\":{id},\"pos\":[{:.3},{:.3}],\"vel\":[{:.3},{:.3}],\"speed\":{:.3}}}",
            p.x,
            p.y,
            v.x,
            v.y,
            v.length()
        ));
    }
    // Machine-readable snapshot: one JSON object so a client can parse a frame in one step.
    let json = format!(
        concat!(
            "{{\"t\":{:.3},\"playfield\":{{\"x\":[{:.3},{:.3}],\"y\":[{:.3},{:.3}]}},",
            "\"bumpers\":[{}],\"kickers\":[{}],\"flippers\":[{}],\"plungers\":[{}],",
            "\"slingshots\":[{}],\"balls\":[{}]}}\n"
        ),
        time.elapsed_secs(),
        -half_w,
        half_w,
        -half_h,
        half_h,
        j_bumpers.join(","),
        j_kickers.join(","),
        j_flippers.join(","),
        j_plungers.join(","),
        j_slingshots.join(","),
        j_balls.join(",")
    );
    let _ = fs::write(&*STATE_JSON_PATH, &json);
    // Also append the frame to the stream so a reader tailing it never misses a frame (e.g. the
    // ball crossing the flipper plane) between two snapshot reads. The line already ends in \n.
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&*STATE_JSONL_PATH)
    {
        let _ = f.write_all(json.as_bytes());
    }
}

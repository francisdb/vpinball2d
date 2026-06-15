# VPinball2D

Visual Pinball 2D engine

## Tables

Tables are read from a folder on the filesystem, defaulting to `~/vpinball/tables`
(override with the `VPINBALL_TABLES` environment variable). The standard Visual
Pinball layout - each table in its own sub-folder alongside its media - is scanned
recursively for `.vpx` files. A good starting point is
the [Visual Pinball example table](https://github.com/vpinball/vpinball/raw/refs/heads/master/src/assets/exampleTable.vpx).

At startup the picker shows only the tables that ship with a script sidecar
(a `.lua` and/or `.table.json` next to the vpx - the curated set that renders
best; see [`examples/`](examples)); use **Show all tables** to browse every
table found on disk in a scrollable list (`*` marks the ones with a sidecar).
Table names come from each `.vpx`'s own metadata, read in the background.

## Build & Run

To build and run the project, make sure you
have [Cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html) installed. Then, execute the following
command in your terminal:

```bash
cargo run --release
```

This launches the **table picker**; choose one to play. Esc returns to the picker,
and Esc in the picker exits the game.

To open a specific table directly - for example when launched by an external
frontend - pass its file name (relative to the tables folder) or a full path to a
`.vpx` file anywhere on disk:

```bash
cargo run --release -- "Total Nuclear Annihilation (Spooky 2017)/Total Nuclear Annihilation (Spooky 2017) VPW v2.3.vpx"
```

In this mode there is no picker, so Esc exits the game.

## Controls

Gameplay:

| Key | Action |
| --- | --- |
| Left Arrow / Left Shift | Left flipper |
| Right Arrow / Right Shift | Right flipper |
| Enter | Plunger (hold to pull back, release to fire) |
| Z | Nudge left |
| / | Nudge right |
| Space | Nudge up (jolt the front of the table) |
| P | Pause / resume |
| Escape | Pause (and back out of menus) |
| Left mouse drag | Drag the ball around (debug aid) |

Developer tools (only in builds with the `dev` feature):

| Key | Action |
| --- | --- |
| ` (backtick) | Toggle the `bevy_ui` debug overlay; outlines menu/UI nodes only, so nothing shows on the playfield during play |
| H | Hide meshes without a collider, leaving just the collision geometry |
| S | Toggle slow motion (1/5 of real time) |

## Play interface

A text control + telemetry channel lets an operator who cannot see the pixels (an AI agent, or
anyone over a terminal) drive and observe the game while a human watches the live window. It is
split into two independent Cargo features:

- `remote_control` - accept commands: move/launch the ball, flippers, plunger, nudge (input)
- `telemetry` - publish game state as JSON plus an event log (read-only)

Enable either or both; the `dev` feature includes both:

```bash
cargo run --features dev                        # both, as part of a dev build
cargo run --features remote_control,telemetry   # both, without the rest of the dev tooling
cargo run --features telemetry                  # read-only observer
```

It communicates through these files in `/tmp`:

| File | Direction | Purpose |
| --- | --- | --- |
| `/tmp/vpinball2d_cmd` | write | newline-separated commands; the game truncates it once read |
| `/tmp/vpinball2d_state.json` | read | latest telemetry frame as one JSON object, overwritten at ~50 Hz |
| `/tmp/vpinball2d_state.jsonl` | read | one JSON object appended per frame; tail it to never miss a frame |
| `/tmp/vpinball2d_events.log` | read | one line appended per ball/object contact |

### Commands

Write commands to `/tmp/vpinball2d_cmd` (one per line). Coordinates are world metres; the
playfield is centred on the origin with `+y` up the table.

| Command | Effect |
| --- | --- |
| `tp <x> <y> <vx> <vy>` | teleport every ball to `(x,y)` with velocity `(vx,vy)`, for a clean repeatable shot |
| `tp <id> <x> <y> <vx> <vy>` | same, but only the ball with that `id` (see telemetry), for multiball tests |
| `launch [speed]` | drop every ball into open playfield from the upper centre at `speed` m/s downward (default 0.5), to start a rally without faking it with `tp` |
| `clear` | despawn all balls |
| `hold <left\|right\|plunge>` | press and keep a flipper/plunger key down |
| `release <left\|right\|plunge>` | release it |
| `tap <left\|right\|plunge> [ms]` | press, then auto-release after `ms` (default 120) |
| `nudge <left\|right\|bottom>` | shake the table once (`bottom` is the front nudge that jolts it upward) |
| `screenshot [path]` | save the current frame to `path` (default `/tmp/vpinball2d_shot.png`) so an operator who cannot see the window can grab one |

Flipper and plunger commands inject into the same keyboard input the gameplay systems read, so
they behave exactly like a real key press.

```bash
echo "tp 0.0 0.45 0.05 0.0" > /tmp/vpinball2d_cmd   # drop a ball into play
echo "tap left 110"         > /tmp/vpinball2d_cmd   # flip the left flipper
echo "screenshot /tmp/shot.png" > /tmp/vpinball2d_cmd   # save the current frame
```

### Headless capture

When there is no display to present a window to (CI, a container, a remote agent), set
`VPINBALL_HEADLESS=1`. The game then runs without a window and renders the main view to an
offscreen image; the `screenshot` command saves that image instead of the (absent) window, so the
rendered output can still be inspected. In a normal windowed run the same command captures the
window.

```bash
VPINBALL_HEADLESS=1 cargo run --features dev &      # render without a window
sleep 6                                             # let the table load
echo "screenshot /tmp/shot.png" > /tmp/vpinball2d_cmd
```

### Telemetry

`/tmp/vpinball2d_state.json` holds the playfield bounds, every flipper/bumper/kicker position, and
the per-ball state as one JSON object (overwritten ~50 Hz) so a client can parse a frame in one
step. Pretty-print it for a human with `jq . /tmp/vpinball2d_state.json`:

```json
{"t":12.340,"playfield":{"x":[-0.257,0.257],"y":[-0.583,0.583]},"bumpers":[{"name":"BumperBumper1","pos":[-0.039,0.226]}],"kickers":[],"flippers":[{"name":"Flipper LeftFlipper","pos":[-0.107,-0.390],"raised":0.000,"pressed":false}],"plungers":[{"name":"Plunger Plunger","pos":[0.229,-0.461],"pulled":0.000}],"balls":[{"id":0,"pos":[0.000,0.450],"vel":[0.050,-0.457],"speed":0.460}]}
```

`t` is the elapsed seconds, so a reader can tell how fresh the snapshot is and detect a stale or
dropped frame. Coordinates are world metres with `+y` up the table. For flippers, `raised` is the
bat angle (0 resting .. 1 fully energised) and `pressed` is the button latch, so a controller can
confirm a `hold`/`tap` engaged rather than inferring it from the ball. For the plunger, `pulled`
is how far it is drawn back (0 .. 1).

Because the snapshot is overwritten in place, a poller can miss a frame (for example the ball
crossing the flipper plane between two reads). `/tmp/vpinball2d_state.jsonl` is the same JSON
frames appended one per line, so a reader can `tail -f` it and process every frame in order. It is
truncated when a gameplay session starts.

`/tmp/vpinball2d_events.log` is a running contact log, e.g. `ball 0 hit BumperBumper2`.

### Notes for automated gameplay

A controller (e.g. an AI agent) that wants to keep the ball alive should drive off the telemetry
predictively, not reactively. Things learned writing one:

- **React off predictions, not the current position.** Telemetry updates at ~50 Hz (20 ms) and a
  poller adds its own latency, so by the time the ball "looks" like it is at the flipper it has
  already dropped past. Extrapolate instead.
- **Predictive flipper.** The flipper bats live at `y` about `-0.385`. Each frame, if the ball is
  descending (`vy < 0`), compute the time to that plane `t = (-0.385 - y) / vy` and the predicted
  crossing `px = x + vx * t`. If `px` is within flipper reach (roughly `-0.15 .. 0.12`), `tap` the
  flipper just before arrival (when `t` is under ~60 ms); pick left/right by `px` against the
  centre (~`-0.02`). Re-arm only after the ball climbs back up (`y > -0.25`) or a short cooldown,
  so you get one decisive flip per descent.
- **Predictive nudge.** Side nudges are weak (~0.04 m/s each), so they only help if applied early.
  Use the same predicted crossing `px`: if it falls *outside* flipper reach the ball is headed for
  an outlane, so start pulsing a recovery nudge (about every 60 ms) while it is still high. The
  ball lurches opposite the shove, so a ball escaping the left -> `nudge left` (ball drifts right,
  back toward a flipper), escaping the right -> `nudge right`.
- **Starting/keeping a rally going.** Use `launch` to put a ball into play (the ball-release lane
  holds it otherwise), and `tp` for exact, repeatable test shots. Watch `state.jsonl` rather than
  the snapshot if you must not miss a frame.

## Known issues

- [avianphysics/avian#990](https://github.com/avianphysics/avian/issues/990): at real (centimetre)
  scale avian's speculative contacts deflect a fast ball off colliders it never touches - including
  geometry buried inside another wall. The contact reach is an absolute distance that does not scale
  with the world, so it shows up at pinball scale near tight features.

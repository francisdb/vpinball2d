//! Static table configuration: the `<table>.table.json` sidecar.
//!
//! Following vpinball#2263, behaviour that needs no game logic - which sound a
//! flipper, slingshot, bumper or the drain makes, which rubbers animate a
//! slingshot - is data, not script. The sidecar maps straight onto the
//! engine's declarative sound/animation resources; a table with a sidecar
//! needs no hand-written Rust table module and its script (if any) stays pure
//! game rules.

use crate::pinball::TablePath;
use crate::pinball::bumper::BumperSounds;
use crate::pinball::flipper::FlipperSounds;
use crate::pinball::gate::GateSounds;
use crate::pinball::kicker::DrainSounds;
use crate::pinball::spinner::SpinnerSounds;
use crate::pinball::targets::TargetSounds;
use crate::pinball::wall::{SlingshotAnimation, SlingshotAnimations, SlingshotSounds};
use crate::screens::Screen;
use bevy::prelude::*;
use serde::Deserialize;

/// The sidecar schema. Every section is optional; present sections insert the
/// matching engine resource. Sound entries are vpx sound names; lists play a
/// random entry per event.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct TableConfig {
    #[serde(default)]
    pub drain: Option<SoundPair>,
    #[serde(default)]
    pub flippers: Option<SoundPair>,
    #[serde(default)]
    pub bumpers: Option<SoundList>,
    #[serde(default)]
    pub slingshots: Option<SoundList>,
    #[serde(default)]
    pub slingshot_animations: Vec<SlingshotAnimationConfig>,
    #[serde(default)]
    pub targets: Option<SoundList>,
    #[serde(default)]
    pub spinners: Option<SoundList>,
    #[serde(default)]
    pub gates: Option<SoundList>,
}

#[derive(Deserialize, Default)]
pub struct SoundList {
    #[serde(default)]
    pub hit: Vec<String>,
}

#[derive(Deserialize, Default)]
pub struct SoundPair {
    #[serde(default)]
    pub down: Vec<String>,
    #[serde(default)]
    pub up: Vec<String>,
}

#[derive(Deserialize)]
pub struct SlingshotAnimationConfig {
    pub slingshot: String,
    pub rest: String,
    pub flexed: String,
}

pub(super) fn plugin(app: &mut App) {
    app.add_systems(OnEnter(Screen::Gameplay), load_sidecar);
}

/// Load `<table>.table.json` next to the vpx and insert the configured
/// resources. Tables with hand-written Rust modules are unaffected unless they
/// also ship a sidecar (the sidecar then wins by running later).
fn load_sidecar(
    mut commands: Commands,
    tables_dir: Option<Res<crate::tables::TablesDir>>,
    table_path: Option<Res<TablePath>>,
) {
    let (Some(tables_dir), Some(table_path)) = (tables_dir, table_path) else {
        return;
    };
    let path = tables_dir
        .0
        .join(&table_path.path)
        .with_extension("table.json");
    let Ok(json) = std::fs::read_to_string(&path) else {
        return;
    };
    let config: TableConfig = match serde_json::from_str(&json) {
        Ok(config) => config,
        Err(e) => {
            warn!("invalid table sidecar {}: {e}", path.display());
            return;
        }
    };
    info!("Loaded table sidecar {}", path.display());

    if let Some(drain) = config.drain {
        commands.insert_resource(DrainSounds {
            drain: drain.down,
            release: drain.up,
        });
    }
    if let Some(flippers) = config.flippers {
        commands.insert_resource(FlipperSounds {
            up: flippers.up,
            down: flippers.down,
        });
    }
    if let Some(bumpers) = config.bumpers {
        commands.insert_resource(BumperSounds { hit: bumpers.hit });
    }
    if let Some(slingshots) = config.slingshots {
        commands.insert_resource(SlingshotSounds {
            hit: slingshots.hit,
        });
    }
    if !config.slingshot_animations.is_empty() {
        commands.insert_resource(SlingshotAnimations(
            config
                .slingshot_animations
                .into_iter()
                .map(|a| SlingshotAnimation {
                    slingshot: a.slingshot,
                    rest: a.rest,
                    flexed: a.flexed,
                })
                .collect(),
        ));
    }
    if let Some(targets) = config.targets {
        commands.insert_resource(TargetSounds { hit: targets.hit });
    }
    if let Some(spinners) = config.spinners {
        commands.insert_resource(SpinnerSounds { spin: spinners.hit });
    }
    if let Some(gates) = config.gates {
        commands.insert_resource(GateSounds { hit: gates.hit });
    }
}

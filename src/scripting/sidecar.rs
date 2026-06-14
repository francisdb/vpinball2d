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
    /// A credit reel: render a textbox's credit value as an animated single-
    /// window reel using a credit strip image, like vpinball's B2S credit
    /// window. For tables whose credit display is a textbox plus a reel image.
    #[serde(default)]
    pub credit_reel: Option<CreditReelConfig>,
}

#[derive(Deserialize)]
pub struct CreditReelConfig {
    /// The textbox whose (numeric) text drives the reel, e.g. `credtxt`.
    pub textbox: String,
    /// The credit strip image (cells 0..=digit_range), e.g. `credreel`.
    pub image: String,
    /// Highest value the strip shows (credreel tops out at 15).
    pub digit_range: i32,
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
    // After spawn_level, which inserts the DesktopLayout the credit reel needs.
    app.add_systems(
        OnEnter(Screen::Gameplay),
        load_sidecar.after(crate::pinball::level::spawn_level),
    );
}

/// Load `<table>.table.json` next to the vpx and insert the configured
/// resources. Tables with hand-written Rust modules are unaffected unless they
/// also ship a sidecar (the sidecar then wins by running later).
#[allow(clippy::too_many_arguments)]
fn load_sidecar(
    mut commands: Commands,
    tables_dir: Option<Res<crate::tables::TablesDir>>,
    table_path: Option<Res<TablePath>>,
    table_assets: Option<Res<crate::pinball::table::TableAssets>>,
    assets_vpx: Res<Assets<crate::vpx::VpxAsset>>,
    mut atlas_layouts: ResMut<Assets<bevy::image::TextureAtlasLayout>>,
    mut images: ResMut<Assets<Image>>,
    desktop_layout: Option<Res<crate::pinball::desktop::DesktopLayout>>,
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
    if let (Some(credit), Some(layout)) = (config.credit_reel, desktop_layout.as_deref()) {
        spawn_credit_reel(
            &mut commands,
            &mut atlas_layouts,
            &mut images,
            table_assets.as_deref(),
            &assets_vpx,
            layout,
            &credit,
        );
    }
}

/// Spawn the credit reel from its config: find the source textbox gameitem for
/// its position, build a single-window reel at it, and tag it so the credit
/// value drives it (see `super::sync_credit_reel`).
#[allow(clippy::too_many_arguments)]
fn spawn_credit_reel(
    commands: &mut Commands,
    atlas_layouts: &mut Assets<bevy::image::TextureAtlasLayout>,
    images: &mut Assets<Image>,
    table_assets: Option<&crate::pinball::table::TableAssets>,
    assets_vpx: &Assets<crate::vpx::VpxAsset>,
    layout: &crate::pinball::desktop::DesktopLayout,
    config: &CreditReelConfig,
) {
    use vpin::vpx::gameitem::GameItemEnum;
    let Some(vpx_asset) = table_assets.and_then(|t| assets_vpx.get(&t.vpx)) else {
        return;
    };
    let Some(textbox) = vpx_asset.raw.gameitems.iter().find_map(|item| match item {
        GameItemEnum::TextBox(tb) if tb.name.eq_ignore_ascii_case(&config.textbox) => Some(tb),
        _ => None,
    }) else {
        warn!("credit_reel textbox '{}' not found", config.textbox);
        return;
    };

    if let Some(entity) = crate::pinball::reel::spawn_credit_reel(
        commands,
        atlas_layouts,
        images,
        vpx_asset,
        layout,
        textbox,
        &config.image,
        config.digit_range,
    ) {
        commands.entity(entity).insert(super::CreditReel {
            textbox: config.textbox.clone(),
            last_value: i64::MIN,
        });
    }
}

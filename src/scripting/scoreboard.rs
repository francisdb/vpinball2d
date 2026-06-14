//! Minimal scoreboard for scripted tables.
//!
//! EM tables display scores on backglass reels and textboxes, which this 2D
//! view does not render. While a table script runs, a side panel shows every
//! reel value and non-empty textbox from the script's shadow state, so the
//! game state (scores, credits, ball in play, match) is visible.

use super::{ScriptRuntime, api::ItemKind, api::ScriptValue};
use crate::pinball::desktop::{DesktopLayout, DesktopText, text_scale};
use crate::screens::Screen;
use bevy::prelude::*;

/// The text node displaying the shadow state.
#[derive(Component)]
pub(super) struct Scoreboard;

pub(super) fn spawn_scoreboard(world: &mut World) {
    world.spawn((
        Name::from("Script scoreboard"),
        Scoreboard,
        Text::new(""),
        TextFont::from_font_size(14.0),
        TextColor(Color::srgb(0.9, 0.85, 0.7)),
        Node {
            position_type: PositionType::Absolute,
            top: px(8),
            right: px(8),
            ..default()
        },
        DespawnOnExit(Screen::Gameplay),
    ));
}

/// Keep the backdrop textbox text in sync with the script's shadow state, fitting
/// each value to its box. A textbox shown by a credit reel is left blank (the reel
/// renders it instead).
pub(super) fn sync_desktop_texts(
    runtime: NonSend<ScriptRuntime>,
    credit_reels: Query<&super::CreditReel>,
    mut texts: Query<(&DesktopText, &mut Text2d, &mut Transform)>,
) {
    let host = runtime.host.borrow();
    for (dt, mut text, mut transform) in &mut texts {
        let value = if credit_reels
            .iter()
            .any(|c| c.textbox.eq_ignore_ascii_case(&dt.name))
        {
            String::new()
        } else {
            match host
                .items
                .get(&dt.name.to_lowercase())
                .and_then(|item| item.props.get("text"))
            {
                Some(ScriptValue::Str(s)) => s.clone(),
                _ => continue,
            }
        };
        if text.0 != value {
            transform.scale = Vec3::splat(text_scale(&value, dt.box_world));
            text.0 = value;
        }
    }
}

pub(super) fn update_scoreboard(
    runtime: NonSend<ScriptRuntime>,
    layout: Option<Res<DesktopLayout>>,
    mut scoreboard: Query<&mut Text, With<Scoreboard>>,
    credit_reels: Query<&super::CreditReel>,
) {
    let Ok(mut text) = scoreboard.single_mut() else {
        return;
    };
    // With a desktop backdrop, the textboxes render there; keep the dev panel empty.
    if layout.is_some_and(|l| l.has_backdrop) {
        if !text.0.is_empty() {
            text.0.clear();
        }
        return;
    }
    let host = runtime.host.borrow();
    // Score reels render in place as animated digit wheels (see pinball::reel),
    // and a credit reel renders its textbox; the panel carries the remaining
    // textboxes (ball in play, match, game over, high score).
    let mut boxes: Vec<(&str, &str)> = Vec::new();
    for item in host.items.values() {
        let on_a_reel = credit_reels
            .iter()
            .any(|c| c.textbox.eq_ignore_ascii_case(&item.name));
        if item.kind == ItemKind::TextBox
            && !on_a_reel
            && let Some(ScriptValue::Str(s)) = item.props.get("text")
            && !s.trim().is_empty()
        {
            boxes.push((item.name.as_str(), s.as_str()));
        }
    }
    boxes.sort_by_key(|(name, _)| *name);
    let joined = boxes
        .iter()
        .map(|(name, text)| format!("{name}: {text}"))
        .collect::<Vec<_>>()
        .join("\n");
    if text.0 != joined {
        text.0 = joined;
    }
}

//! Minimal scoreboard for scripted tables.
//!
//! EM tables display scores on backglass reels and textboxes, which this 2D
//! view does not render. While a table script runs, a side panel shows every
//! reel value and non-empty textbox from the script's shadow state, so the
//! game state (scores, credits, ball in play, match) is visible.

use super::{ScriptRuntime, api::ItemKind, api::ScriptValue};
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

pub(super) fn update_scoreboard(
    runtime: NonSend<ScriptRuntime>,
    mut scoreboard: Query<&mut Text, With<Scoreboard>>,
) {
    let Ok(mut text) = scoreboard.single_mut() else {
        return;
    };
    let host = runtime.host.borrow();
    let mut reels: Vec<(&str, i64)> = Vec::new();
    let mut boxes: Vec<(&str, &str)> = Vec::new();
    for item in host.items.values() {
        match item.kind {
            ItemKind::Reel => {
                let value = item
                    .props
                    .get("value")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                reels.push((item.name.as_str(), value));
            }
            ItemKind::TextBox => {
                if let Some(ScriptValue::Str(s)) = item.props.get("text")
                    && !s.trim().is_empty()
                {
                    boxes.push((item.name.as_str(), s.as_str()));
                }
            }
            _ => {}
        }
    }
    reels.sort_by_key(|(name, _)| *name);
    boxes.sort_by_key(|(name, _)| *name);
    let mut lines: Vec<String> = reels
        .iter()
        .map(|(name, value)| format!("{name}: {value:06}"))
        .collect();
    lines.extend(boxes.iter().map(|(name, text)| format!("{name}: {text}")));
    let joined = lines.join("\n");
    if text.0 != joined {
        text.0 = joined;
    }
}

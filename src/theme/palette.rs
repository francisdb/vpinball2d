use bevy::prelude::*;

/// #ddd369
pub const LABEL_TEXT: Color = Color::srgb(0.867, 0.827, 0.412);

/// #fcfbcc
pub const HEADER_TEXT: Color = Color::srgb(0.988, 0.984, 0.800);

/// #ececec
pub const BUTTON_TEXT: Color = Color::srgb(0.925, 0.925, 0.925);
/// #4666bf
pub const BUTTON_BACKGROUND: Color = Color::srgb(0.275, 0.400, 0.750);
/// #6299d1
pub const BUTTON_HOVERED_BACKGROUND: Color = Color::srgb(0.384, 0.600, 0.820);
/// #3d4999
pub const BUTTON_PRESSED_BACKGROUND: Color = Color::srgb(0.239, 0.286, 0.600);
/// #80692b - resting background for a selected/highlighted button
pub const BUTTON_SELECTED_BACKGROUND: Color = Color::srgb(0.502, 0.412, 0.169);

/// Scrollbar track: a dark, subtle gutter behind the thumb.
pub const SCROLLBAR_TRACK: Color = Color::srgba(0.0, 0.0, 0.0, 0.25);
/// Scrollbar thumb: the draggable handle, in the button accent blue.
pub const SCROLLBAR_THUMB: Color = BUTTON_BACKGROUND;

//! A Rust port of FlexDMD (vpinball's `plugins/flexdmd`), driven by table
//! scripts through the engine-agnostic script bridge.
//!
//! FlexDMD is a small retained-mode scene graph (a tree of [`Actor`]s rooted at
//! a "Stage" group) that a table script builds and mutates; each frame the tree
//! is updated and rasterised into a `width`x`height` DMD image (128x32 by
//! default). The script never renders - it only mutates this logical model via
//! the bridge (see `scripting`), and a Bevy system ([`render`]) does the
//! compositing and shows the result in the backbox.
//!
//! This module holds the pure data model + typed mutators (no Bevy, no script
//! types), so it can be unit-tested and so neither the script bridge nor the
//! renderer leak into it. Rendering and script wiring live in [`render`] and
//! `scripting::flexdmd` respectively.

// On wasm the Lua bridge (which drives the scene graph) is gated out, so the
// model's mutators go unused there; allow it like `scripting` does.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

pub mod render;

/// Index of an [`Actor`] in [`FlexDmd::actors`]; the script's opaque handle.
pub type ActorId = usize;

/// How an [`ActorKind::Image`] (or animated actor) fits its bounds. Mirrors
/// FlexDMD's `Scaling` (actors/Layout.h). The full set is ported; only `Stretch`
/// is set so far (until the script-side `Scaling` property is wired).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[allow(dead_code)]
pub enum Scaling {
    #[default]
    Stretch,
    Fill,
    FillX,
    FillY,
    None,
}

/// Anchor for positioning/aligning content within bounds. Mirrors FlexDMD's
/// `Alignment` (actors/Layout.h). Full set ported; only `Center` is set so far.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[allow(dead_code)]
pub enum Alignment {
    TopLeft,
    Top,
    TopRight,
    Left,
    #[default]
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Alignment {
    /// `(fx, fy)` in `[0,1]`: fraction of the free space placed before the
    /// content on each axis (0 = start, 0.5 = centre, 1 = end).
    pub fn fractions(self) -> (f32, f32) {
        use Alignment::*;
        let fx = match self {
            TopLeft | Left | BottomLeft => 0.0,
            Top | Center | Bottom => 0.5,
            TopRight | Right | BottomRight => 1.0,
        };
        let fy = match self {
            TopLeft | Top | TopRight => 0.0,
            Left | Center | Right => 0.5,
            BottomLeft | Bottom | BottomRight => 1.0,
        };
        (fx, fy)
    }
}

/// The per-kind state of an [`Actor`].
#[derive(Debug)]
pub enum ActorKind {
    /// A container; draws its children in insertion order, optionally clipped.
    Group { children: Vec<ActorId>, clip: bool },
    /// A bitmap, resolved from `src` (e.g. `"VPX.d_border"`, `"VPX.x&dmd=2&add"`).
    Image {
        src: String,
        scaling: Scaling,
        alignment: Alignment,
    },
    /// A border/fill rectangle. Colours are `0x00RRGGBB`.
    Frame {
        thickness: i32,
        border_color: u32,
        fill: bool,
        fill_color: u32,
    },
    /// Bitmap-font text (rendered by [`render`] once fonts are supported).
    Label { font: String, text: String },
}

/// One node of the scene graph.
#[derive(Debug)]
pub struct Actor {
    pub name: String,
    pub kind: ActorKind,
    pub x: f32,
    pub y: f32,
    pub width: i32,
    pub height: i32,
    /// "Natural" size (image/text size); used by `pack` and alignment.
    pub pref_width: i32,
    pub pref_height: i32,
    pub visible: bool,
    pub fill_parent: bool,
    pub clear_background: bool,
    pub parent: Option<ActorId>,
}

impl Actor {
    fn new(name: String, kind: ActorKind) -> Self {
        Self {
            name,
            kind,
            x: 0.0,
            y: 0.0,
            width: 0,
            height: 0,
            pref_width: 0,
            pref_height: 0,
            visible: true,
            fill_parent: false,
            clear_background: false,
            parent: None,
        }
    }
}

/// The FlexDMD instance: the scene graph plus display config. One per table.
#[derive(Debug)]
pub struct FlexDmd {
    pub width: u32,
    pub height: u32,
    /// 0=GRAY_2, 1=GRAY_4, 2=RGB (default 1 in FlexDMD; tables set 2).
    pub render_mode: i32,
    pub run: bool,
    pub show: bool,
    pub clear: bool,
    pub game_name: String,
    /// Tint applied to gray output by the consumer; `0x00RRGGBB`.
    pub color: u32,
    /// While > 0, the renderer leaves the frame untouched (script batch edits).
    pub lock_count: i32,
    /// Bumped on any change; the renderer redraws when it differs.
    pub revision: u64,
    actors: Vec<Actor>,
    stage: ActorId,
}

impl Default for FlexDmd {
    fn default() -> Self {
        let actors = vec![Actor::new(
            "Stage".to_string(),
            ActorKind::Group {
                children: Vec::new(),
                clip: false,
            },
        )];
        Self {
            width: 128,
            height: 32,
            render_mode: 1,
            run: false,
            show: true,
            clear: true,
            game_name: String::new(),
            color: 0x00_20_58_ff, // FlexDMD default orange (RGB low-byte = R)
            lock_count: 0,
            revision: 0,
            actors,
            stage: 0,
        }
    }
}

impl FlexDmd {
    pub fn stage(&self) -> ActorId {
        self.stage
    }
    pub fn actor(&self, id: ActorId) -> Option<&Actor> {
        self.actors.get(id)
    }
    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    fn add_actor(&mut self, actor: Actor) -> ActorId {
        let id = self.actors.len();
        self.actors.push(actor);
        self.touch();
        id
    }

    /// `FlexDMD.NewGroup(name)`.
    pub fn new_group(&mut self, name: &str) -> ActorId {
        self.add_actor(Actor::new(
            name.to_string(),
            ActorKind::Group {
                children: Vec::new(),
                clip: false,
            },
        ))
    }

    /// `FlexDMD.NewImage(name, src)`. `pref_*` is filled by the renderer once it
    /// resolves the bitmap (we have no image sizes here).
    pub fn new_image(&mut self, name: &str, src: &str) -> ActorId {
        self.add_actor(Actor::new(
            name.to_string(),
            ActorKind::Image {
                src: src.to_string(),
                scaling: Scaling::Stretch,
                alignment: Alignment::Center,
            },
        ))
    }

    /// `FlexDMD.NewFrame(name)`.
    pub fn new_frame(&mut self, name: &str) -> ActorId {
        self.add_actor(Actor::new(
            name.to_string(),
            ActorKind::Frame {
                thickness: 2,
                border_color: 0x00_ff_ff_ff,
                fill: false,
                fill_color: 0,
            },
        ))
    }

    /// `FlexDMD.NewLabel(name, font, text)`.
    pub fn new_label(&mut self, name: &str, font: &str, text: &str) -> ActorId {
        self.add_actor(Actor::new(
            name.to_string(),
            ActorKind::Label {
                font: font.to_string(),
                text: text.to_string(),
            },
        ))
    }

    /// `group.AddActor(child)` - reparent and append.
    pub fn group_add(&mut self, group: ActorId, child: ActorId) {
        // Detach from a previous parent first.
        if let Some(old) = self.actors.get(child).and_then(|a| a.parent) {
            self.group_remove(old, child);
        }
        if let Some(actor) = self.actors.get_mut(child) {
            actor.parent = Some(group);
        }
        if let Some(Actor {
            kind: ActorKind::Group { children, .. },
            ..
        }) = self.actors.get_mut(group)
        {
            children.push(child);
        }
        self.touch();
    }

    /// `group.RemoveActor(child)`.
    pub fn group_remove(&mut self, group: ActorId, child: ActorId) {
        if let Some(Actor {
            kind: ActorKind::Group { children, .. },
            ..
        }) = self.actors.get_mut(group)
        {
            children.retain(|&c| c != child);
        }
        if let Some(actor) = self.actors.get_mut(child)
            && actor.parent == Some(group)
        {
            actor.parent = None;
        }
        self.touch();
    }

    /// `group.GetXxx(name)` - depth-first search by name (FlexDMD runtime <=1008
    /// semantics, which the target tables use). Optionally filter by kind.
    pub fn group_find(&self, group: ActorId, name: &str) -> Option<ActorId> {
        let actor = self.actors.get(group)?;
        if actor.name == name {
            return Some(group);
        }
        if let ActorKind::Group { children, .. } = &actor.kind {
            for &child in children {
                if let Some(found) = self.group_find(child, name) {
                    return Some(found);
                }
            }
        }
        None
    }

    pub fn set_bounds(&mut self, id: ActorId, x: f32, y: f32, w: i32, h: i32) {
        if let Some(a) = self.actors.get_mut(id) {
            a.x = x;
            a.y = y;
            a.width = w;
            a.height = h;
            self.touch();
        }
    }

    pub fn set_position(&mut self, id: ActorId, x: f32, y: f32) {
        if let Some(a) = self.actors.get_mut(id) {
            a.x = x;
            a.y = y;
            self.touch();
        }
    }

    pub fn set_size(&mut self, id: ActorId, w: i32, h: i32) {
        if let Some(a) = self.actors.get_mut(id) {
            a.width = w;
            a.height = h;
            self.touch();
        }
    }

    /// Set a numeric actor property by (lowercased) name.
    pub fn set_actor_num(&mut self, id: ActorId, prop: &str, value: f64) {
        let Some(a) = self.actors.get_mut(id) else {
            return;
        };
        match prop {
            "x" => a.x = value as f32,
            "y" => a.y = value as f32,
            "width" => a.width = value as i32,
            "height" => a.height = value as i32,
            "prefwidth" => a.pref_width = value as i32,
            "prefheight" => a.pref_height = value as i32,
            "thickness" => {
                if let ActorKind::Frame { thickness, .. } = &mut a.kind {
                    *thickness = value as i32;
                }
            }
            "bordercolor" => {
                if let ActorKind::Frame { border_color, .. } = &mut a.kind {
                    *border_color = value as u32;
                }
            }
            "fillcolor" => {
                if let ActorKind::Frame { fill_color, .. } = &mut a.kind {
                    *fill_color = value as u32;
                }
            }
            _ => {}
        }
        self.touch();
    }

    pub fn set_actor_bool(&mut self, id: ActorId, prop: &str, value: bool) {
        let Some(a) = self.actors.get_mut(id) else {
            return;
        };
        match prop {
            "visible" => a.visible = value,
            "fillparent" => a.fill_parent = value,
            "clearbackground" => a.clear_background = value,
            "fill" => {
                if let ActorKind::Frame { fill, .. } = &mut a.kind {
                    *fill = value;
                }
            }
            "clip" => {
                if let ActorKind::Group { clip, .. } = &mut a.kind {
                    *clip = value;
                }
            }
            _ => {}
        }
        self.touch();
    }

    pub fn set_actor_str(&mut self, id: ActorId, prop: &str, value: &str) {
        let Some(a) = self.actors.get_mut(id) else {
            return;
        };
        match prop {
            "name" => a.name = value.to_string(),
            // `image.Bitmap = other.Bitmap`: Bitmap reads/writes the src token.
            "bitmap" | "src" => {
                if let ActorKind::Image { src, .. } = &mut a.kind {
                    *src = value.to_string();
                }
            }
            "text" => {
                if let ActorKind::Label { text, .. } = &mut a.kind {
                    *text = value.to_string();
                }
            }
            "font" => {
                if let ActorKind::Label { font, .. } = &mut a.kind {
                    *font = value.to_string();
                }
            }
            _ => {}
        }
        self.touch();
    }

    pub fn get_actor_num(&self, id: ActorId, prop: &str) -> Option<f64> {
        let a = self.actors.get(id)?;
        Some(match prop {
            "x" => a.x as f64,
            "y" => a.y as f64,
            "width" => a.width as f64,
            "height" => a.height as f64,
            "prefwidth" => a.pref_width as f64,
            "prefheight" => a.pref_height as f64,
            _ => return None,
        })
    }

    pub fn get_actor_bool(&self, id: ActorId, prop: &str) -> Option<bool> {
        let a = self.actors.get(id)?;
        Some(match prop {
            "visible" => a.visible,
            "fillparent" => a.fill_parent,
            _ => return None,
        })
    }

    /// `image.Bitmap` / `label.Text` etc. as a string token.
    pub fn get_actor_str(&self, id: ActorId, prop: &str) -> Option<String> {
        let a = self.actors.get(id)?;
        Some(match (&a.kind, prop) {
            (ActorKind::Image { src, .. }, "bitmap" | "src") => src.clone(),
            (ActorKind::Label { text, .. }, "text") => text.clone(),
            _ => a.name.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_add_find_reparent() {
        let mut d = FlexDmd::default();
        let scene = d.new_group("Scene");
        let back = d.new_image("Back", "VPX.d_border");
        d.group_add(scene, back);
        d.group_add(d.stage(), scene);
        assert_eq!(d.group_find(d.stage(), "Back"), Some(back));
        // Reparent moves it out of the old group.
        let other = d.new_group("Other");
        d.group_add(other, back);
        assert_eq!(d.group_find(scene, "Back"), None);
        assert_eq!(d.group_find(other, "Back"), Some(back));
    }

    #[test]
    fn bitmap_token_copies_src() {
        let mut d = FlexDmd::default();
        let a = d.new_image("a", "VPX.glyph_X&dmd=2&add");
        let b = d.new_image("b", "VPX.d_empty&dmd=2");
        let token = d.get_actor_str(a, "bitmap").unwrap();
        d.set_actor_str(b, "bitmap", &token);
        assert_eq!(
            d.get_actor_str(b, "bitmap").unwrap(),
            "VPX.glyph_X&dmd=2&add"
        );
    }
}

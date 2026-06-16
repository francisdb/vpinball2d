//! The engine-agnostic script bridge.
//!
//! A table script (Lua today, JavaScript later) talks to the engine through a
//! deliberately tiny FFI surface - get/set an item property, call an item
//! method, call a host function - so a new language binding only has to expose
//! those four entry points plus event dispatch. Everything else (the
//! vpinball-style object model with `light.state = 1` and `kicker:kick(0, 18)`)
//! is sugar built *inside* the scripting language by a per-language prelude.
//!
//! Scripts never touch the ECS directly: property writes and method calls are
//! queued as [`ScriptCommand`]s that Bevy systems apply, and property reads come
//! from a shadow [`HostState`] the runtime keeps in sync (write-through on set,
//! seeded from the vpx data on load). This keeps the engine free to run the
//! script at any point in the frame without re-entrancy.

use bevy::platform::collections::HashMap;
use std::cell::RefCell;
use std::rc::Rc;

/// A dynamically-typed value crossing the script boundary.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptValue {
    Nil,
    Bool(bool),
    Int(i64),
    Num(f64),
    Str(String),
}

impl ScriptValue {
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            ScriptValue::Int(i) => Some(*i as f32),
            ScriptValue::Num(n) => Some(*n as f32),
            ScriptValue::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ScriptValue::Bool(b) => Some(*b),
            ScriptValue::Int(i) => Some(*i != 0),
            ScriptValue::Num(n) => Some(*n != 0.0),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            ScriptValue::Int(i) => Some(*i),
            ScriptValue::Num(n) => Some(*n as i64),
            _ => None,
        }
    }
}

/// What kind of table item a script name refers to; decides which commands its
/// property writes and method calls translate to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ItemKind {
    Light,
    Kicker,
    Timer,
    Flipper,
    Plunger,
    Wall,
    Trigger,
    Bumper,
    Spinner,
    Target,
    TextBox,
    Reel,
    Flasher,
    #[default]
    Other,
}

/// Shadow of one item: the properties the script has read/write access to.
#[derive(Debug, Default)]
pub struct ItemState {
    pub kind: ItemKind,
    /// Canonical (original-case) item name, for command targets and display.
    pub name: String,
    pub props: HashMap<String, ScriptValue>,
}

/// A side effect the script asked for, applied by Bevy systems after dispatch.
#[derive(Debug, Clone)]
pub enum ScriptCommand {
    /// `item.prop = value`; the shadow state is already updated, this is the
    /// ECS side. The runtime routes on (kind, prop).
    SetProp {
        name: String,
        prop: String,
        value: ScriptValue,
    },
    /// `item:method(args)`, e.g. a kicker kick.
    Call {
        name: String,
        method: String,
        args: Vec<ScriptValue>,
    },
    /// Global host functions (play_sound etc.).
    PlaySound {
        name: String,
    },
    StopSound {
        name: String,
    },
    SetFlippersEnabled(bool),
}

/// State shared between the script engine (via `Rc` captured in its host
/// closures) and the Bevy runtime that drains it once per frame.
#[derive(Default)]
pub struct HostState {
    /// Queued side effects, in script call order.
    pub commands: Vec<ScriptCommand>,
    /// Shadow item state, keyed by lowercase name (vbscript heritage: table
    /// scripts are case-insensitive about item names).
    pub items: HashMap<String, ItemState>,
    /// Per-table persistent key/value store (high scores, credits).
    pub store: HashMap<String, String>,
    /// Whether the store changed and should be written back to disk.
    pub store_dirty: bool,
    /// The table's FlexDMD scene graph, built and mutated by the script and
    /// rasterised by `crate::flexdmd::render`.
    pub flexdmd: crate::flexdmd::FlexDmd,
}

impl HostState {
    pub fn item(&self, name: &str) -> Option<&ItemState> {
        self.items.get(&name.to_lowercase())
    }

    /// Write-through property set: updates the shadow and queues the command.
    pub fn set_prop(&mut self, name: &str, prop: &str, value: ScriptValue) {
        let key = name.to_lowercase();
        let prop = prop.to_lowercase();
        if let Some(item) = self.items.get_mut(&key) {
            item.props.insert(prop.clone(), value.clone());
            let canonical = item.name.clone();
            self.commands.push(ScriptCommand::SetProp {
                name: canonical,
                prop,
                value,
            });
        }
    }

    pub fn get_prop(&self, name: &str, prop: &str) -> ScriptValue {
        self.item(name)
            .and_then(|item| item.props.get(&prop.to_lowercase()).cloned())
            .unwrap_or(ScriptValue::Nil)
    }

    pub fn call(&mut self, name: &str, method: &str, args: Vec<ScriptValue>) {
        if let Some(item) = self.item(name) {
            let canonical = item.name.clone();
            self.commands.push(ScriptCommand::Call {
                name: canonical,
                method: method.to_lowercase(),
                args,
            });
        }
    }
}

/// Handle to the shared host state, captured by the engine's host closures.
pub type SharedHost = Rc<RefCell<HostState>>;

#[derive(Debug, thiserror::Error)]
#[error("script error: {0}")]
pub struct ScriptError(pub String);

/// A scripting language binding. Implementations exist for Lua ([`super::lua`]);
/// a QuickJS-based JavaScript engine can implement the same trait against the
/// same [`SharedHost`] - the FFI surface is the four host entry points the
/// prelude uses, nothing engine-specific leaks out.
pub trait ScriptEngine {
    /// Compile and run the table script (top-level code runs once).
    fn load(&mut self, source: &str) -> Result<(), ScriptError>;
    /// Call the global event handler `name` if the script defines one.
    /// Returns whether a handler ran.
    fn dispatch(&mut self, name: &str, args: &[ScriptValue]) -> Result<bool, ScriptError>;
}

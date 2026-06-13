//! Lua binding of the script bridge (mlua, Lua 5.4).
//!
//! Exposes exactly the four host entry points from [`super::api`] as globals
//! (`__get`, `__set`, `__call_item`, `__host`) plus the item/collection lists,
//! then runs `prelude.lua` which builds the vpinball-style object model on top.
//! A future JavaScript engine binds the same four functions and ships its own
//! prelude; nothing else changes.

use super::api::{ScriptEngine, ScriptError, ScriptValue, SharedHost};
use bevy::log::{info, warn};
use mlua::{Lua, MultiValue, Value, Variadic};

const PRELUDE: &str = include_str!("prelude.lua");

pub struct LuaEngine {
    lua: Lua,
}

fn to_lua(lua: &Lua, value: &ScriptValue) -> mlua::Result<Value> {
    Ok(match value {
        ScriptValue::Nil => Value::Nil,
        ScriptValue::Bool(b) => Value::Boolean(*b),
        ScriptValue::Int(i) => Value::Integer(*i),
        ScriptValue::Num(n) => Value::Number(*n),
        ScriptValue::Str(s) => Value::String(lua.create_string(s)?),
    })
}

fn from_lua(value: &Value) -> ScriptValue {
    match value {
        Value::Boolean(b) => ScriptValue::Bool(*b),
        Value::Integer(i) => ScriptValue::Int(*i),
        Value::Number(n) => ScriptValue::Num(*n),
        Value::String(s) => ScriptValue::Str(s.to_string_lossy().to_string()),
        _ => ScriptValue::Nil,
    }
}

impl LuaEngine {
    /// Build the engine over the shared host state and run the prelude.
    /// `collections` maps a collection name to its member item names.
    pub fn new(
        host: SharedHost,
        collections: &[(String, Vec<String>)],
    ) -> Result<Self, ScriptError> {
        let lua = Lua::new();
        let wrap = |e: mlua::Error| ScriptError(e.to_string());

        {
            let globals = lua.globals();

            let h = host.clone();
            let get = lua
                .create_function(move |lua, (name, prop): (String, String)| {
                    to_lua(lua, &h.borrow().get_prop(&name, &prop))
                })
                .map_err(wrap)?;
            globals.set("__get", get).map_err(wrap)?;

            let h = host.clone();
            let set = lua
                .create_function(move |_, (name, prop, value): (String, String, Value)| {
                    h.borrow_mut().set_prop(&name, &prop, from_lua(&value));
                    Ok(())
                })
                .map_err(wrap)?;
            globals.set("__set", set).map_err(wrap)?;

            let h = host.clone();
            let call = lua
                .create_function(
                    move |_, (name, method, args): (String, String, Variadic<Value>)| {
                        let args = args.iter().map(from_lua).collect();
                        h.borrow_mut().call(&name, &method, args);
                        Ok(())
                    },
                )
                .map_err(wrap)?;
            globals.set("__call_item", call).map_err(wrap)?;

            // Global host functions, multiplexed on the first argument so the
            // FFI surface stays a single entry point.
            let h = host.clone();
            let host_fn = lua
                .create_function(move |lua, (what, args): (String, Variadic<Value>)| {
                    let mut host = h.borrow_mut();
                    match what.as_str() {
                        "play_sound" => {
                            if let Some(Value::String(s)) = args.first() {
                                let name = s.to_string_lossy().to_string();
                                host.commands
                                    .push(super::api::ScriptCommand::PlaySound { name });
                            }
                            Ok(Value::Nil)
                        }
                        "stop_sound" => {
                            if let Some(Value::String(s)) = args.first() {
                                let name = s.to_string_lossy().to_string();
                                host.commands
                                    .push(super::api::ScriptCommand::StopSound { name });
                            }
                            Ok(Value::Nil)
                        }
                        "set_flippers_enabled" => {
                            let enabled = args.first().map(from_lua).and_then(|v| v.as_bool());
                            host.commands
                                .push(super::api::ScriptCommand::SetFlippersEnabled(
                                    enabled.unwrap_or(true),
                                ));
                            Ok(Value::Nil)
                        }
                        "store_get" => {
                            let key = match args.first() {
                                Some(Value::String(s)) => s.to_string_lossy().to_string(),
                                _ => return Ok(Value::Nil),
                            };
                            match host.store.get(&key) {
                                Some(v) => Ok(Value::String(lua.create_string(v)?)),
                                None => Ok(Value::Nil),
                            }
                        }
                        "store_set" => {
                            if let (Some(Value::String(k)), Some(v)) = (args.first(), args.get(1)) {
                                let value = match v {
                                    Value::String(s) => s.to_string_lossy().to_string(),
                                    Value::Integer(i) => i.to_string(),
                                    Value::Number(n) => n.to_string(),
                                    Value::Boolean(b) => b.to_string(),
                                    _ => String::new(),
                                };
                                host.store.insert(k.to_string_lossy().to_string(), value);
                                host.store_dirty = true;
                            }
                            Ok(Value::Nil)
                        }
                        "log" => {
                            if let Some(Value::String(s)) = args.first() {
                                info!("script: {}", s.to_string_lossy());
                            }
                            Ok(Value::Nil)
                        }
                        other => {
                            warn!("script called unknown host function '{other}'");
                            Ok(Value::Nil)
                        }
                    }
                })
                .map_err(wrap)?;
            globals.set("__host", host_fn).map_err(wrap)?;

            // The item registry (lowercase name -> kind tag) for the prelude's
            // global proxies, and the table's collections as name lists.
            let items = lua.create_table().map_err(wrap)?;
            for key in host.borrow().items.keys() {
                items.set(key.as_str(), true).map_err(wrap)?;
            }
            globals.set("__items", items).map_err(wrap)?;

            let cols = lua.create_table().map_err(wrap)?;
            for (name, members) in collections {
                let list = lua.create_table().map_err(wrap)?;
                for (i, member) in members.iter().enumerate() {
                    list.set(i + 1, member.as_str()).map_err(wrap)?;
                }
                cols.set(name.as_str(), list).map_err(wrap)?;
            }
            globals.set("__collections", cols).map_err(wrap)?;
        }

        lua.load(PRELUDE)
            .set_name("prelude.lua")
            .exec()
            .map_err(wrap)?;

        Ok(Self { lua })
    }
}

impl ScriptEngine for LuaEngine {
    fn load(&mut self, source: &str) -> Result<(), ScriptError> {
        self.lua
            .load(source)
            .set_name("table script")
            .exec()
            .map_err(|e| ScriptError(e.to_string()))
    }

    fn dispatch(&mut self, name: &str, args: &[ScriptValue]) -> Result<bool, ScriptError> {
        let globals = self.lua.globals();
        let Ok(handler) = globals.get::<mlua::Function>(name) else {
            return Ok(false);
        };
        let mut lua_args = MultiValue::new();
        for arg in args {
            lua_args.push_back(to_lua(&self.lua, arg).map_err(|e| ScriptError(e.to_string()))?);
        }
        handler
            .call::<()>(lua_args)
            .map_err(|e| ScriptError(e.to_string()))?;
        Ok(true)
    }
}

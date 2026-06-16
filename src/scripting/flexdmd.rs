//! Bridge between the script `__host` FFI and the [`crate::flexdmd`] scene graph.
//!
//! The Lua/JS prelude turns `FlexDMD = CreateObject("FlexDMD.FlexDMD")`,
//! `g:AddActor(img)`, `img.Bitmap = ...` into flat `__host("fd_*"/"actor_*", ...)`
//! calls; this routes them to typed [`crate::flexdmd::FlexDmd`] mutators, returning actor
//! handles (integers) so the prelude can wrap them as proxy objects. Keeping the
//! translation here keeps `lua.rs` engine-specific and `flexdmd` script-agnostic.

use super::api::HostState;
use mlua::{Lua, Value};

fn i(args: &[Value], n: usize) -> i64 {
    match args.get(n) {
        Some(Value::Integer(v)) => *v,
        Some(Value::Number(v)) => *v as i64,
        _ => 0,
    }
}
fn f(args: &[Value], n: usize) -> f64 {
    match args.get(n) {
        Some(Value::Integer(v)) => *v as f64,
        Some(Value::Number(v)) => *v,
        _ => 0.0,
    }
}
fn s(args: &[Value], n: usize) -> String {
    match args.get(n) {
        Some(Value::String(v)) => v.to_string_lossy().to_string(),
        _ => String::new(),
    }
}
fn id(args: &[Value], n: usize) -> usize {
    i(args, n).max(0) as usize
}

/// Handle a `__host` call if it targets FlexDMD; returns `Some(result)` when it
/// did (so `lua.rs` stops there), `None` to fall through to other host fns.
pub(super) fn host_op(
    lua: &Lua,
    host: &mut HostState,
    what: &str,
    args: &[Value],
) -> Option<mlua::Result<Value>> {
    let dmd = &mut host.flexdmd;
    let out = match what {
        // FlexDMD-level property set.
        "fd_set" => {
            let prop = s(args, 0);
            match prop.as_str() {
                "width" => dmd.width = f(args, 1).max(1.0) as u32,
                "height" => dmd.height = f(args, 1).max(1.0) as u32,
                "rendermode" => dmd.render_mode = i(args, 1) as i32,
                "color" => dmd.color = i(args, 1) as u32,
                "run" => dmd.run = matches!(args.get(1), Some(Value::Boolean(true))),
                "show" => dmd.show = matches!(args.get(1), Some(Value::Boolean(true))),
                "clear" => dmd.clear = matches!(args.get(1), Some(Value::Boolean(true))),
                "gamename" => dmd.game_name = s(args, 1),
                "tablefile" | "projectfolder" | "runtimeversion" | "segments" => {}
                _ => {}
            }
            Value::Nil
        }
        // FlexDMD-level property get.
        "fd_get" => match s(args, 0).as_str() {
            "stage" => Value::Integer(dmd.stage() as i64),
            "width" => Value::Integer(dmd.width as i64),
            "height" => Value::Integer(dmd.height as i64),
            "run" => Value::Boolean(dmd.run),
            "show" => Value::Boolean(dmd.show),
            "version" => Value::Integer(1009),
            "runtimeversion" => Value::Integer(1008),
            _ => Value::Nil,
        },
        "fd_new_group" => Value::Integer(dmd.new_group(&s(args, 0)) as i64),
        "fd_new_image" => Value::Integer(dmd.new_image(&s(args, 0), &s(args, 1)) as i64),
        "fd_new_frame" => Value::Integer(dmd.new_frame(&s(args, 0)) as i64),
        "fd_new_label" => {
            Value::Integer(dmd.new_label(&s(args, 0), &s(args, 1), &s(args, 2)) as i64)
        }
        "fd_lock" => {
            dmd.lock_count += 1;
            Value::Nil
        }
        "fd_unlock" => {
            dmd.lock_count = (dmd.lock_count - 1).max(0);
            Value::Nil
        }

        // Group ops.
        "group_add" => {
            dmd.group_add(id(args, 0), id(args, 1));
            Value::Nil
        }
        "group_remove" => {
            dmd.group_remove(id(args, 0), id(args, 1));
            Value::Nil
        }
        "group_find" => match dmd.group_find(id(args, 0), &s(args, 1)) {
            Some(found) => Value::Integer(found as i64),
            None => Value::Nil,
        },

        // Actor property get/set + geometry.
        "actor_set_num" => {
            dmd.set_actor_num(id(args, 0), &s(args, 1), f(args, 2));
            Value::Nil
        }
        "actor_set_bool" => {
            dmd.set_actor_bool(
                id(args, 0),
                &s(args, 1),
                matches!(args.get(2), Some(Value::Boolean(true))),
            );
            Value::Nil
        }
        "actor_set_str" => {
            dmd.set_actor_str(id(args, 0), &s(args, 1), &s(args, 2));
            Value::Nil
        }
        "actor_get_num" => match dmd.get_actor_num(id(args, 0), &s(args, 1)) {
            Some(v) => Value::Number(v),
            None => Value::Nil,
        },
        "actor_get_bool" => match dmd.get_actor_bool(id(args, 0), &s(args, 1)) {
            Some(v) => Value::Boolean(v),
            None => Value::Nil,
        },
        "actor_get_str" => match dmd.get_actor_str(id(args, 0), &s(args, 1)) {
            Some(v) => match lua.create_string(&v) {
                Ok(st) => Value::String(st),
                Err(e) => return Some(Err(e)),
            },
            None => Value::Nil,
        },
        "actor_set_bounds" => {
            dmd.set_bounds(
                id(args, 0),
                f(args, 1) as f32,
                f(args, 2) as f32,
                i(args, 3) as i32,
                i(args, 4) as i32,
            );
            Value::Nil
        }
        "actor_set_position" => {
            dmd.set_position(id(args, 0), f(args, 1) as f32, f(args, 2) as f32);
            Value::Nil
        }
        "actor_set_size" => {
            dmd.set_size(id(args, 0), i(args, 1) as i32, i(args, 2) as i32);
            Value::Nil
        }
        _ => return None,
    };
    Some(Ok(out))
}

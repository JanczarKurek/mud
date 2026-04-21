//! `mud_quest_api` pymodule — the surface Python quest scripts see.
//!
//! Functions are pure FFI into a thread-local/single-threaded static bridge:
//! before dispatching into a Python hook, the engine installs a
//! `QuestApiContext` (active player + a borrowed `QuestApiEffects` outbox);
//! the hook runs and any `player_give`, `set_var`, etc. calls write into the
//! outbox; the engine drains that outbox and replays the effects as Bevy
//! commands / variable-store writes.
//!
//! This mirrors the console VM's `mud_api` bridge pattern but with a separate
//! static so the two VMs don't stomp each other.

use std::cell::RefCell;

use crate::dialog::variable_storage::PersistentVariableStorage;
use crate::dialog::variable_storage::YarnValueDump;

thread_local! {
    static ACTIVE_CONTEXT: RefCell<Option<ActiveContext>> = const { RefCell::new(None) };
}

/// Effects queued by `mud_quest_api` calls during one Python hook invocation.
/// Drained and applied by the caller (quest systems) after the hook returns.
#[derive(Default)]
pub struct QuestApiEffects {
    pub give: Vec<(String, u32)>,
    pub take: Vec<(String, u32)>,
    pub quest_complete: Vec<String>,
    pub quest_fail: Vec<String>,
    pub log_lines: Vec<String>,
}

/// Context available while a Python hook is running. `var_store` is the
/// per-character Yarn variable store (shared Arc); writes from Python land in
/// the same store Yarn reads from.
pub struct ActiveContext<'a> {
    pub player_id: u64,
    pub var_store: Option<PersistentVariableStorage>,
    pub inventory: std::collections::HashMap<String, u32>,
    pub effects: &'a mut QuestApiEffects,
}

/// Runs `f` with `ctx` installed in the thread-local. Used by the quest
/// systems to fence each Python call with the right player state. `f`
/// typically goes on to call into Python via `Interpreter::enter`.
///
/// Only the outermost caller should install context — the engine's hook
/// invocations deliberately don't re-wrap, so that nested Python calls
/// during a single frame's dispatch all see the same `var_store`, `effects`,
/// and inventory.
pub fn with_full_call_context<R>(
    player_id: u64,
    var_store: Option<PersistentVariableStorage>,
    inventory: std::collections::HashMap<String, u32>,
    effects: &mut QuestApiEffects,
    f: impl FnOnce() -> R,
) -> R {
    let ctx = ActiveContext {
        player_id,
        var_store,
        inventory,
        effects,
    };
    with_context(ctx, |_| f())
}

fn with_context<R>(ctx: ActiveContext<'_>, f: impl FnOnce(&()) -> R) -> R {
    // SAFETY: we extend the lifetime of `ctx` to `'static` for storage in the
    // thread-local. The RefCell is cleared before this function returns, so
    // no one can observe the dangling lifetime.
    let ctx_static: ActiveContext<'static> = unsafe { std::mem::transmute(ctx) };
    ACTIVE_CONTEXT.with(|cell| {
        *cell.borrow_mut() = Some(ctx_static);
    });
    struct ClearOnDrop;
    impl Drop for ClearOnDrop {
        fn drop(&mut self) {
            ACTIVE_CONTEXT.with(|cell| {
                *cell.borrow_mut() = None;
            });
        }
    }
    let _guard = ClearOnDrop;
    f(&())
}

fn with_ctx<R>(f: impl FnOnce(&mut ActiveContext<'static>) -> R) -> Option<R> {
    ACTIVE_CONTEXT.with(|cell| cell.borrow_mut().as_mut().map(|ctx| f(ctx)))
}

#[rustpython_vm::pymodule]
pub mod mud_quest_api {
    use bevy_yarnspinner::prelude::{VariableStorage, YarnValue};
    use rustpython_vm::convert::ToPyObject;
    use rustpython_vm::{PyObjectRef, PyResult, VirtualMachine};

    use super::{with_ctx, YarnValueDump};

    #[pyfunction]
    fn log(message: String) {
        with_ctx(|ctx| ctx.effects.log_lines.push(message));
    }

    #[pyfunction]
    fn player_id() -> u64 {
        with_ctx(|ctx| ctx.player_id).unwrap_or(0)
    }

    #[pyfunction]
    fn set_var(name: String, value: PyObjectRef, vm: &VirtualMachine) -> PyResult<()> {
        let dump = pyobject_to_yarn_value(&value, vm)?;
        with_ctx(|ctx| {
            let yarn_name = ensure_dollar(&name);
            if let Some(store) = ctx.var_store.as_ref() {
                // `PersistentVariableStorage::set` takes &mut self but the
                // underlying state is Arc<RwLock<...>>, so a cheap clone is
                // sufficient. Yarn's `<<if $foo>>` reads the same state.
                let mut store_clone = store.clone();
                let yarn_value: YarnValue = dump.into();
                if let Err(err) = store_clone.set(yarn_name, yarn_value) {
                    bevy::log::warn!("mud_quest_api.set_var failed: {err}");
                }
            }
        });
        Ok(())
    }

    #[pyfunction]
    fn get_var(name: String, vm: &VirtualMachine) -> PyObjectRef {
        let yarn_name = ensure_dollar(&name);
        with_ctx(|ctx| {
            let Some(store) = ctx.var_store.as_ref() else {
                return vm.ctx.none();
            };
            match store.snapshot().get(&yarn_name) {
                Some(YarnValueDump::Number(n)) => (*n).to_pyobject(vm),
                Some(YarnValueDump::String(s)) => s.clone().to_pyobject(vm),
                Some(YarnValueDump::Boolean(b)) => (*b).to_pyobject(vm),
                None => vm.ctx.none(),
            }
        })
        .unwrap_or_else(|| vm.ctx.none())
    }

    #[pyfunction]
    fn player_has(args: rustpython_vm::function::FuncArgs, vm: &VirtualMachine) -> bool {
        let type_id: String = args
            .args
            .first()
            .and_then(|v| v.clone().try_into_value::<String>(vm).ok())
            .unwrap_or_default();
        let count: u32 = args
            .args
            .get(1)
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
            .map(|n| n.max(0) as u32)
            .unwrap_or(1);
        with_ctx(|ctx| {
            ctx.inventory
                .get(&type_id)
                .is_some_and(|total| *total >= count)
        })
        .unwrap_or(false)
    }

    #[pyfunction]
    fn player_give(args: rustpython_vm::function::FuncArgs, vm: &VirtualMachine) {
        let (type_id, count) = parse_item_count(&args, 1, vm);
        with_ctx(|ctx| ctx.effects.give.push((type_id, count)));
    }

    #[pyfunction]
    fn player_take(args: rustpython_vm::function::FuncArgs, vm: &VirtualMachine) {
        let (type_id, count) = parse_item_count(&args, 1, vm);
        with_ctx(|ctx| ctx.effects.take.push((type_id, count)));
    }

    #[pyfunction]
    fn complete_quest(quest_id: String) {
        with_ctx(|ctx| ctx.effects.quest_complete.push(quest_id));
    }

    #[pyfunction]
    fn fail_quest(quest_id: String) {
        with_ctx(|ctx| ctx.effects.quest_fail.push(quest_id));
    }

    fn ensure_dollar(name: &str) -> String {
        if name.starts_with('$') {
            name.to_owned()
        } else {
            format!("${name}")
        }
    }

    fn parse_item_count(
        args: &rustpython_vm::function::FuncArgs,
        default_count: u32,
        vm: &VirtualMachine,
    ) -> (String, u32) {
        let type_id: String = args
            .args
            .first()
            .and_then(|v| v.clone().try_into_value::<String>(vm).ok())
            .unwrap_or_default();
        let count: u32 = args
            .args
            .get(1)
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
            .map(|n| n.max(0) as u32)
            .unwrap_or(default_count);
        (type_id, count)
    }

    fn pyobject_to_yarn_value(
        obj: &PyObjectRef,
        vm: &VirtualMachine,
    ) -> PyResult<YarnValueDump> {
        let _ = YarnValue::Boolean(false); // keep import alive
        if let Ok(b) = obj.clone().try_into_value::<bool>(vm) {
            return Ok(YarnValueDump::Boolean(b));
        }
        if let Ok(n) = obj.clone().try_into_value::<i64>(vm) {
            return Ok(YarnValueDump::Number(n as f32));
        }
        if let Ok(n) = obj.clone().try_into_value::<f64>(vm) {
            return Ok(YarnValueDump::Number(n as f32));
        }
        if let Ok(s) = obj.clone().try_into_value::<String>(vm) {
            return Ok(YarnValueDump::String(s));
        }
        Err(vm.new_type_error(
            "mud_quest_api: yarn vars must be str, int, float, or bool".to_owned(),
        ))
    }
}

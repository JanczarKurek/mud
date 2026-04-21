//! Dedicated RustPython VM for quest scripts. Separate from the dev console VM
//! so experimentation there can't affect quest state, and vice versa.
//!
//! Each `.py` file in `assets/quests/` is loaded into the VM under its own
//! globals scope. The quest "name" is the filename stem (`demo_hunter.py` →
//! `"demo_hunter"`). Modules may export:
//!
//! - `state: dict` — default state, deep-copied into per-character storage on
//!   `<<start_quest>>`.
//! - `subscribes_to: list[str]` — event kinds the quest reacts to. Absent or
//!   empty = no event forwarding (zero per-frame overhead).
//! - `on_start(state)` / `on_event(ev, state)` / `on_command(name, args, state)`
//!   — lifecycle hooks. Any of them may be omitted; the engine only calls what
//!   exists.

use std::collections::HashMap;
use std::fs;
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use rustpython::InterpreterConfig;
use rustpython_vm::builtins::PyDict;
use rustpython_vm::convert::ToPyObject;
use rustpython_vm::function::FuncArgs;
use rustpython_vm::scope::Scope;
use rustpython_vm::{Interpreter, PyObjectRef, VirtualMachine};

use crate::quest::events::QuestEvent;
use crate::quest::python;

/// Stored per loaded quest module.
pub struct QuestDef {
    pub name: String,
    pub scope: Scope,
    pub default_state: Option<PyObjectRef>,
    pub subscribes_to: Vec<String>,
    pub on_start: Option<PyObjectRef>,
    pub on_event: Option<PyObjectRef>,
    pub on_command: Option<PyObjectRef>,
}

pub struct QuestEngine {
    pub interpreter: ManuallyDrop<Interpreter>,
    pub quests: HashMap<String, QuestDef>,
    pub active_states: HashMap<(u64, String), PyObjectRef>,
    /// Reverse index: event kind → quest ids that care. Built once at load.
    pub subs_by_kind: HashMap<String, Vec<String>>,
}

impl QuestEngine {
    pub fn new() -> Self {
        let interpreter = InterpreterConfig::new()
            .init_stdlib()
            .add_native_module(
                "mud_quest_api".to_owned(),
                python::mud_quest_api::make_module,
            )
            .interpreter();

        Self {
            interpreter: ManuallyDrop::new(interpreter),
            quests: HashMap::new(),
            active_states: HashMap::new(),
            subs_by_kind: HashMap::new(),
        }
    }

    /// Walk `assets/quests/*.py` and load each into its own scope. Errors are
    /// logged per-file; one bad quest shouldn't prevent the others from
    /// loading.
    pub fn load_from(&mut self, dir: &Path) {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                info!("quest dir {} not present; no quests loaded", dir.display());
                return;
            }
            Err(err) => {
                warn!("failed to read quest dir {}: {err}", dir.display());
                return;
            }
        };

        let mut files: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("py"))
            .collect();
        files.sort();

        for path in files {
            if let Err(err) = self.load_file(&path) {
                warn!("failed to load quest {}: {err}", path.display());
            }
        }

        self.rebuild_subscriptions();
        info!(
            "quest engine loaded {} quests ({} subscriptions)",
            self.quests.len(),
            self.subs_by_kind.values().map(|v| v.len()).sum::<usize>()
        );
    }

    fn load_file(&mut self, path: &Path) -> Result<(), String> {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| "invalid filename".to_owned())?
            .to_owned();
        let source = fs::read_to_string(path).map_err(|e| e.to_string())?;

        let (scope, default_state, subscribes_to, on_start, on_event, on_command) =
            self.interpreter.enter(|vm| {
                let scope = vm.new_scope_with_builtins();
                vm.run_code_string(scope.clone(), &source, path.display().to_string())
                    .map_err(|e| format_py_error(vm, &e))?;

                let globals = scope.globals.clone();
                let default_state = globals.get_item("state", vm).ok();
                let subscribes_to = read_subscribes_to(&globals, vm);
                let on_start = globals.get_item("on_start", vm).ok();
                let on_event = globals.get_item("on_event", vm).ok();
                let on_command = globals.get_item("on_command", vm).ok();

                Ok::<_, String>((
                    scope,
                    default_state,
                    subscribes_to,
                    on_start,
                    on_event,
                    on_command,
                ))
            })?;

        info!(
            "quest loaded: {name} (subscribes_to={:?}, hooks: start={} event={} command={})",
            subscribes_to,
            on_start.is_some(),
            on_event.is_some(),
            on_command.is_some()
        );

        self.quests.insert(
            name.clone(),
            QuestDef {
                name,
                scope,
                default_state,
                subscribes_to,
                on_start,
                on_event,
                on_command,
            },
        );
        Ok(())
    }

    fn rebuild_subscriptions(&mut self) {
        self.subs_by_kind.clear();
        for (quest_id, def) in &self.quests {
            for kind in &def.subscribes_to {
                self.subs_by_kind
                    .entry(kind.clone())
                    .or_default()
                    .push(quest_id.clone());
            }
        }
    }

    /// Start a quest for a player: deep-copy the module's default `state`
    /// dict, stash it, and invoke `on_start(state)` if defined. No-op if the
    /// player already has active state for this quest.
    ///
    /// The caller is responsible for installing a `mud_quest_api` context
    /// (via `python::with_full_call_context`) that wraps this call — this
    /// method invokes the hook directly without nesting another context.
    pub fn start_quest(&mut self, player_id: u64, quest_id: &str) -> bool {
        let key = (player_id, quest_id.to_owned());
        if self.active_states.contains_key(&key) {
            return false;
        }
        let Some(def) = self.quests.get(quest_id) else {
            warn!("start_quest: unknown quest '{quest_id}'");
            return false;
        };
        let on_start = def.on_start.clone();
        let default_state = def.default_state.clone();

        let state = self.interpreter.enter(|vm| {
            let state = match &default_state {
                Some(default) => deep_copy(default, vm).unwrap_or_else(|| new_empty_dict(vm)),
                None => new_empty_dict(vm),
            };
            if let Some(on_start) = on_start {
                if let Err(err) = invoke_hook(vm, on_start, vec![state.clone()]) {
                    warn!("quest '{quest_id}' on_start failed: {err}");
                }
            }
            state
        });

        self.active_states.insert(key, state);
        let _ = player_id;
        true
    }

    /// Invoke the module's `on_command(name, args, state)`. Requires the
    /// player to have active state for this quest (call `start_quest` first).
    pub fn dispatch_command(
        &mut self,
        player_id: u64,
        quest_id: &str,
        name: &str,
        args: Vec<String>,
    ) {
        let Some(state) = self
            .active_states
            .get(&(player_id, quest_id.to_owned()))
            .cloned()
        else {
            warn!("dispatch_command: no active state for player {player_id} quest '{quest_id}'");
            return;
        };
        let Some(def) = self.quests.get(quest_id) else {
            return;
        };
        let Some(on_command) = def.on_command.clone() else {
            return;
        };

        self.interpreter.enter(|vm| {
            let py_name = name.to_pyobject(vm);
            let py_args = vm
                .ctx
                .new_list(args.into_iter().map(|s| s.to_pyobject(vm)).collect())
                .into();
            if let Err(err) = invoke_hook(vm, on_command, vec![py_name, py_args, state]) {
                warn!("quest '{quest_id}' on_command failed: {err}");
            }
        });
    }

    /// Fan a `QuestEvent` out to every (player, quest) pair that subscribes
    /// to its kind AND has active state. No-op if `subs_by_kind` has no
    /// entry — this is the "no firehose" short-circuit.
    ///
    /// Caller must install context for each `player_id` we yield; see
    /// `dispatch_event_for_player` for the single-player variant.
    pub fn dispatch_event_for_player(
        &mut self,
        event: &QuestEvent,
        player_id: u64,
    ) {
        let kind = event.kind();
        let Some(quest_ids) = self.subs_by_kind.get(kind).cloned() else {
            return;
        };

        for quest_id in quest_ids {
            let Some(state) = self
                .active_states
                .get(&(player_id, quest_id.clone()))
                .cloned()
            else {
                continue;
            };
            let Some(def) = self.quests.get(&quest_id) else {
                continue;
            };
            let Some(on_event) = def.on_event.clone() else {
                continue;
            };

            self.interpreter.enter(|vm| {
                let ev_dict = event_to_pydict(event, vm);
                if let Err(err) = invoke_hook(vm, on_event, vec![ev_dict, state]) {
                    warn!("quest '{quest_id}' on_event failed: {err}");
                }
            });
        }
    }

    /// Remove active state — used by `complete_quest` / `fail_quest` API.
    pub fn end_quest(&mut self, player_id: u64, quest_id: &str) {
        self.active_states.remove(&(player_id, quest_id.to_owned()));
    }
}

impl Default for QuestEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for QuestEngine {
    fn drop(&mut self) {
        // Same rationale as PythonConsoleHost: RustPython teardown hangs or
        // crashes on shutdown; the OS reclaims memory either way.
    }
}

fn read_subscribes_to(globals: &rustpython_vm::builtins::PyDictRef, vm: &VirtualMachine) -> Vec<String> {
    let Ok(value) = globals.get_item("subscribes_to", vm) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Ok(iter) = value.get_iter(vm) {
        while let rustpython_vm::protocol::PyIterReturn::Return(item) =
            iter.next(vm).unwrap_or(rustpython_vm::protocol::PyIterReturn::StopIteration(None))
        {
            if let Ok(s) = item.try_to_value::<String>(vm) {
                out.push(s);
            }
        }
    }
    out
}

fn new_empty_dict(vm: &VirtualMachine) -> PyObjectRef {
    PyDict::new_ref(&vm.ctx).into()
}

/// Deep-copy via Python's `copy.deepcopy`.
fn deep_copy(obj: &PyObjectRef, vm: &VirtualMachine) -> Option<PyObjectRef> {
    let copy_module = vm.import("copy", 0).ok()?;
    let deepcopy = copy_module.get_attr("deepcopy", vm).ok()?;
    deepcopy.call((obj.clone(),), vm).ok()
}

fn invoke_hook(
    vm: &VirtualMachine,
    callable: PyObjectRef,
    args: Vec<PyObjectRef>,
) -> Result<PyObjectRef, String> {
    callable
        .call(FuncArgs::from(args), vm)
        .map_err(|e| format_py_error(vm, &e))
}

fn format_py_error(vm: &VirtualMachine, err: &rustpython_vm::PyRef<rustpython_vm::builtins::PyBaseException>) -> String {
    let mut buf = String::new();
    vm.write_exception(&mut buf, err).ok();
    buf
}

fn event_to_pydict(event: &QuestEvent, vm: &VirtualMachine) -> PyObjectRef {
    let dict = PyDict::new_ref(&vm.ctx);
    let kind = event.kind().to_pyobject(vm);
    dict.set_item("kind", kind, vm).ok();
    match event {
        QuestEvent::ObjectKilled {
            type_id,
            killer_player_id,
        } => {
            dict.set_item("type_id", type_id.clone().to_pyobject(vm), vm).ok();
            let killer: PyObjectRef = match killer_player_id {
                Some(id) => id.to_pyobject(vm),
                None => vm.ctx.none(),
            };
            dict.set_item("killer_player_id", killer, vm).ok();
        }
    }
    dict.into()
}

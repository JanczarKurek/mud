//! Admin Python console — embedded RustPython VM, persistent scope, exposes
//! the shared `world` API surface from `crate::scripting_api`.
//!
//! Each `execute()` call builds an `AdminApiContext` from a fresh
//! `WorldSnapshot`, installs it for the duration of the Python invocation,
//! and returns the queued `GameCommand`s plus styled output lines.

use std::mem::ManuallyDrop;
use std::sync::{Arc, Mutex};

use bevy_terminal::LineStyle;
use rustpython::InterpreterConfig;
use rustpython_vm::scope::Scope;
use rustpython_vm::Interpreter;

use crate::game::commands::GameCommand;
use crate::player::components::PlayerId;
use crate::scripting_api::bindings::world_api;
use crate::scripting_api::{install_ctx, ApiContext, ApiError, WorldSnapshot};

/// Bootstrap shim — runs once when the VM is first created and after each
/// explicit `world.reset()`. Aliases the legacy module name and rebinds
/// `print` to route through `world.log` so output lands in the console.
///
/// Collection args are pretty-printed via `pprint.pformat` so a
/// `print(world.objects())` becomes hundreds of short lines instead of one
/// multi-kilobyte string — Bevy's text-layout pipeline is dramatically
/// faster on short spans, and the output is readable in the bargain.
const BOOTSTRAP_SCRIPT: &str = r#"
import world
import sys
import pprint
sys.modules['mud_api'] = world

def _mud_format(arg):
    if isinstance(arg, (list, tuple, dict, set, frozenset)):
        return pprint.pformat(arg, width=120)
    return str(arg)

def _mud_print(*args, sep=" ", end=""):
    world.log(sep.join(_mud_format(arg) for arg in args) + end)

print = _mud_print
"#;

#[derive(Default)]
struct AdminContextInner {
    commands: Vec<GameCommand>,
    targeted_commands: Vec<(PlayerId, GameCommand)>,
    log_lines: Vec<String>,
    reset_pending: bool,
}

pub struct AdminApiContext {
    snapshot: WorldSnapshot,
    inner: Mutex<AdminContextInner>,
}

impl AdminApiContext {
    pub fn new(snapshot: WorldSnapshot) -> Self {
        Self {
            snapshot,
            inner: Mutex::new(AdminContextInner::default()),
        }
    }
}

impl ApiContext for AdminApiContext {
    fn is_admin(&self) -> bool {
        true
    }

    fn caller_player_id(&self) -> Option<u64> {
        self.snapshot.local_player_id
    }

    fn snapshot(&self) -> &WorldSnapshot {
        &self.snapshot
    }

    fn log(&self, message: String) {
        let mut inner = self.inner.lock().expect("admin api context poisoned");
        inner.log_lines.push(message);
    }

    fn queue_command(&self, command: GameCommand) -> Result<(), ApiError> {
        let mut inner = self.inner.lock().expect("admin api context poisoned");
        inner.commands.push(command);
        Ok(())
    }

    fn queue_command_for_player(
        &self,
        target: PlayerId,
        command: GameCommand,
    ) -> Result<(), ApiError> {
        let mut inner = self.inner.lock().expect("admin api context poisoned");
        inner.targeted_commands.push((target, command));
        Ok(())
    }

    fn reset_scope(&self) -> Result<(), ApiError> {
        let mut inner = self.inner.lock().expect("admin api context poisoned");
        inner.reset_pending = true;
        Ok(())
    }
}

/// Result of running a single REPL submission.
#[derive(Default, Debug)]
pub struct PythonExecOutput {
    pub lines: Vec<(String, LineStyle)>,
    pub commands: Vec<GameCommand>,
    pub targeted_commands: Vec<(PlayerId, GameCommand)>,
}

pub struct PythonConsoleHost {
    interpreter: ManuallyDrop<Interpreter>,
    scope: ManuallyDrop<Scope>,
}

impl PythonConsoleHost {
    pub fn new() -> Self {
        let interpreter = InterpreterConfig::new()
            .init_stdlib()
            .add_native_module("world".to_owned(), world_api::make_module)
            .interpreter();

        let scope = Self::build_scope(&interpreter);

        Self {
            interpreter: ManuallyDrop::new(interpreter),
            scope: ManuallyDrop::new(scope),
        }
    }

    fn build_scope(interpreter: &Interpreter) -> Scope {
        interpreter.enter(|vm| {
            let scope = vm.new_scope_with_builtins();
            vm.run_code_string(scope.clone(), BOOTSTRAP_SCRIPT, "<mud-bootstrap>".into())
                .expect("Failed to initialize embedded Python console");
            scope
        })
    }

    /// Run one Python input string in the persistent scope. Returns the
    /// queued `GameCommand`s the script produced plus the styled output
    /// lines (caller forwards them to the terminal widget and
    /// `PendingGameCommands`).
    pub fn execute(&mut self, command: &str, snapshot: WorldSnapshot) -> PythonExecOutput {
        let context = Arc::new(AdminApiContext::new(snapshot));
        let trait_ctx: Arc<dyn ApiContext> = context.clone();

        let result = install_ctx(trait_ctx, || {
            self.interpreter.enter(|vm| {
                vm.run_code_string((*self.scope).clone(), command, "<mud-console>".into())
            })
        });

        let mut output = PythonExecOutput::default();

        if let Err(error) = result {
            output
                .lines
                .push((format!("Python error: {error:?}"), LineStyle::Traceback));
        }

        let (queued_commands, targeted_commands, log_lines, reset_pending) = {
            let mut inner = context.inner.lock().expect("admin api context poisoned");
            (
                std::mem::take(&mut inner.commands),
                std::mem::take(&mut inner.targeted_commands),
                std::mem::take(&mut inner.log_lines),
                std::mem::replace(&mut inner.reset_pending, false),
            )
        };

        for line in log_lines {
            output.lines.push((line, LineStyle::Stdout));
        }

        if reset_pending {
            self.reset_scope();
            output.lines.push((
                "[System] world.reset(): scope cleared.".to_owned(),
                LineStyle::System,
            ));
        }

        output.commands = queued_commands;
        output.targeted_commands = targeted_commands;
        output
    }

    /// Drop the persistent scope and rebuild a fresh one. Same observable
    /// behaviour as `world.reset()` from within the REPL — exposed as a
    /// method so the UI "Restart" button can trigger it directly.
    pub fn reset_scope(&mut self) {
        let new_scope = Self::build_scope(&self.interpreter);
        // ManuallyDrop bookkeeping: drop the old scope before overwriting
        // the slot so we don't leak. Safe because nothing else holds a
        // reference to the Scope (the VM keeps its own internal handles
        // through globals/locals on the interpreter state, not via this
        // ManuallyDrop wrapper).
        unsafe {
            ManuallyDrop::drop(&mut self.scope);
            self.scope = ManuallyDrop::new(new_scope);
        }
    }

    /// Return identifiers in the persistent scope whose name starts with
    /// `prefix`. Used to power Tab completion.
    pub fn complete_prefix(&self, prefix: &str) -> Vec<String> {
        let prefix_owned = prefix.to_owned();
        self.interpreter.enter(|vm| {
            let globals = self.scope.globals.clone();
            let mut matches: Vec<String> = (&*globals)
                .into_iter()
                .filter_map(|(key, _value)| {
                    let s = key.str(vm).ok()?.as_str().to_owned();
                    if s.starts_with(&prefix_owned) {
                        Some(s)
                    } else {
                        None
                    }
                })
                .collect();
            matches.sort();
            matches.dedup();
            matches
        })
    }
}

impl Drop for PythonConsoleHost {
    fn drop(&mut self) {
        // RustPython teardown currently hangs or crashes on application
        // shutdown. Intentionally leaking the VM state is acceptable here
        // because the process is already exiting and the OS will reclaim
        // the memory.
    }
}

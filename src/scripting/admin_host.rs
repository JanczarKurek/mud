//! Admin Python REPL host — used by `network/admin.rs` to evaluate Python
//! input arriving over the admin UNIX socket. Sibling to
//! [`crate::scripting::python::PythonConsoleHost`], but with `sys.stdout` and
//! `sys.displayhook` redirected so that REPL-style expression evaluation
//! (`>>> 1+1` → `2`) prints results without an explicit `print(...)` call.
//!
//! Compilation is split from execution so the polling system can detect
//! "input incomplete" (e.g. the user has typed `def foo():` and is mid-block)
//! and emit a `... ` continuation prompt instead of running prematurely.

use std::mem::ManuallyDrop;
use std::sync::{Arc, Mutex};

use rustpython::InterpreterConfig;
use rustpython_vm::builtins::PyCode;
use rustpython_vm::compiler::Mode;
use rustpython_vm::scope::Scope;
use rustpython_vm::{Interpreter, PyRef};

use crate::game::commands::GameCommand;
use crate::scripting_api::bindings::world_api;
use crate::scripting_api::{install_ctx, ApiContext, ApiError, WorldSnapshot};

/// Bootstrap shim run once per host construction. Reroutes `print`,
/// `sys.stdout`, `sys.stderr`, and `sys.displayhook` through `world.log` so
/// the polling system can capture everything the REPL emits.
const ADMIN_BOOTSTRAP_SCRIPT: &str = r#"
import world
import sys
sys.modules['mud_api'] = world

class _AdminWriter:
    def __init__(self):
        self._buf = ""
    def write(self, s):
        if not isinstance(s, str):
            s = str(s)
        self._buf += s
        while "\n" in self._buf:
            line, self._buf = self._buf.split("\n", 1)
            world.log(line)
        return len(s)
    def flush(self):
        if self._buf:
            world.log(self._buf)
            self._buf = ""

sys.stdout = _AdminWriter()
sys.stderr = _AdminWriter()

def _admin_displayhook(value):
    if value is None:
        return
    try:
        world.log(repr(value))
    except Exception:
        world.log(str(value))

sys.displayhook = _admin_displayhook

def _admin_print(*args, sep=" ", end="\n"):
    text = sep.join(str(arg) for arg in args) + end
    if text.endswith("\n"):
        text = text[:-1]
    if text:
        world.log(text)

print = _admin_print
"#;

#[derive(Default)]
struct AdminContextInner {
    commands: Vec<GameCommand>,
    log_lines: Vec<String>,
    /// Set by `world.attach_player(id)` — the polling system reads this
    /// back after a call returns and stashes it on the session so future
    /// inputs from the same connection use the new caller.
    pending_attach: Option<Option<u64>>,
}

pub struct AdminReplApiContext {
    snapshot: WorldSnapshot,
    caller: Option<u64>,
    inner: Mutex<AdminContextInner>,
}

impl AdminReplApiContext {
    pub fn new(snapshot: WorldSnapshot, caller: Option<u64>) -> Self {
        Self {
            snapshot,
            caller,
            inner: Mutex::new(AdminContextInner::default()),
        }
    }
}

impl ApiContext for AdminReplApiContext {
    fn is_admin(&self) -> bool {
        true
    }

    fn caller_player_id(&self) -> Option<u64> {
        self.caller
    }

    fn snapshot(&self) -> &WorldSnapshot {
        &self.snapshot
    }

    fn log(&self, message: String) {
        let mut inner = self.inner.lock().expect("admin repl ctx poisoned");
        inner.log_lines.push(message);
    }

    fn queue_command(&self, command: GameCommand) -> Result<(), ApiError> {
        let mut inner = self.inner.lock().expect("admin repl ctx poisoned");
        inner.commands.push(command);
        Ok(())
    }

    fn attach_player(&self, player_id: Option<u64>) -> Result<(), ApiError> {
        let mut inner = self.inner.lock().expect("admin repl ctx poisoned");
        inner.pending_attach = Some(player_id);
        Ok(())
    }
}

pub enum CompileOutcome {
    Complete(PyRef<PyCode>),
    Incomplete,
    SyntaxError(String),
}

#[derive(Default)]
pub struct AdminExecResult {
    pub stdout: Vec<String>,
    pub error: Option<String>,
    pub queued_commands: Vec<GameCommand>,
    /// `Some(Some(id))` ⇒ session's caller becomes that player.
    /// `Some(None)`     ⇒ session's caller is cleared.
    /// `None`           ⇒ no change.
    pub attach: Option<Option<u64>>,
}

pub struct AdminReplHost {
    interpreter: ManuallyDrop<Interpreter>,
    scope: ManuallyDrop<Scope>,
}

impl AdminReplHost {
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
            vm.run_code_string(
                scope.clone(),
                ADMIN_BOOTSTRAP_SCRIPT,
                "<admin-bootstrap>".to_owned(),
            )
            .expect("Failed to bootstrap admin Python REPL");
            scope
        })
    }

    /// Try to compile `src` as `Mode::Single` (the canonical interactive-REPL
    /// compile mode — a top-level expression auto-invokes `sys.displayhook`).
    /// On a syntax error whose message looks like "input is incomplete" we
    /// return [`CompileOutcome::Incomplete`] so the caller can buffer and
    /// emit a continuation prompt.
    ///
    /// Convention: a trailing blank line (`\n\n`) is the user's explicit
    /// "execute now" signal — when present, any compile error is reported
    /// as a real `SyntaxError` instead of asking for more input.
    pub fn compile_or_incomplete(&self, src: &str) -> CompileOutcome {
        self.interpreter.enter(
            |vm| match vm.compile(src, Mode::Single, "<admin>".to_owned()) {
                Ok(code) => CompileOutcome::Complete(code),
                Err(err) => {
                    let msg = format!("{err}");
                    if is_incomplete_input(src, &msg) {
                        CompileOutcome::Incomplete
                    } else {
                        CompileOutcome::SyntaxError(msg)
                    }
                }
            },
        )
    }

    pub fn execute_compiled(
        &mut self,
        code: PyRef<PyCode>,
        snapshot: WorldSnapshot,
        caller: Option<u64>,
    ) -> AdminExecResult {
        let context = Arc::new(AdminReplApiContext::new(snapshot, caller));
        let trait_ctx: Arc<dyn ApiContext> = context.clone();

        let result = install_ctx(trait_ctx, || {
            self.interpreter
                .enter(|vm| vm.run_code_obj(code, (*self.scope).clone()))
        });

        let error = result.err().map(|py_err| {
            self.interpreter.enter(|vm| {
                let mut buf = String::new();
                vm.write_exception(&mut buf, &py_err).ok();
                buf.trim_end().to_owned()
            })
        });

        let (queued_commands, log_lines, attach) = {
            let mut inner = context.inner.lock().expect("admin repl ctx poisoned");
            (
                std::mem::take(&mut inner.commands),
                std::mem::take(&mut inner.log_lines),
                inner.pending_attach.take(),
            )
        };

        AdminExecResult {
            stdout: log_lines,
            error,
            queued_commands,
            attach,
        }
    }
}

impl Default for AdminReplHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AdminReplHost {
    fn drop(&mut self) {
        // RustPython teardown is unreliable on exit (see the matching note on
        // `PythonConsoleHost::drop`). Intentionally leak — process is
        // exiting; OS reclaims the memory.
    }
}

fn is_incomplete_input(src: &str, message: &str) -> bool {
    // A trailing blank line is the user's "execute now" trigger — once
    // they've typed it, any compile error is real, not a sign of more input
    // to come.
    if src.ends_with("\n\n") || src.ends_with("\r\n\r\n") {
        return false;
    }
    let lower = message.to_ascii_lowercase();
    lower.contains("eof")
        || lower.contains("unexpected end")
        || lower.contains("expected an indented block")
        || lower.contains("incomplete input")
        || lower.contains("unindent")
        || lower.contains("dedent")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_compiles_as_complete_no_op() {
        let host = AdminReplHost::new();
        match host.compile_or_incomplete("") {
            CompileOutcome::Complete(_) => {}
            CompileOutcome::Incomplete => panic!("empty input should be complete (no-op)"),
            CompileOutcome::SyntaxError(msg) => panic!("syntax error on empty input: {msg}"),
        }
    }

    #[test]
    fn simple_expression_is_complete() {
        let host = AdminReplHost::new();
        match host.compile_or_incomplete("1+1\n") {
            CompileOutcome::Complete(_) => {}
            other => panic!("expected Complete for `1+1`, got {:?}", other.label()),
        }
    }

    #[test]
    fn def_without_body_is_incomplete() {
        let host = AdminReplHost::new();
        match host.compile_or_incomplete("def f():\n") {
            CompileOutcome::Incomplete => {}
            CompileOutcome::SyntaxError(msg) => {
                panic!("expected Incomplete for `def f():`, got SyntaxError: {msg}")
            }
            CompileOutcome::Complete(_) => {
                panic!("expected Incomplete for `def f():`, got Complete")
            }
        }
    }

    #[test]
    fn def_with_body_is_complete() {
        let host = AdminReplHost::new();
        match host.compile_or_incomplete("def f():\n    return 1\n\n") {
            CompileOutcome::Complete(_) => {}
            other => panic!("expected Complete for full def, got {:?}", other.label()),
        }
    }

    #[test]
    fn genuine_syntax_error_is_reported() {
        let host = AdminReplHost::new();
        match host.compile_or_incomplete(")(") {
            CompileOutcome::SyntaxError(_) => {}
            other => panic!("expected SyntaxError, got {:?}", other.label()),
        }
    }

    #[test]
    fn execute_simple_expression_captures_repr_via_displayhook() {
        let mut host = AdminReplHost::new();
        let snapshot = WorldSnapshot::default();
        let code = match host.compile_or_incomplete("1+1\n") {
            CompileOutcome::Complete(c) => c,
            other => panic!("compile failed: {:?}", other.label()),
        };
        let result = host.execute_compiled(code, snapshot, None);
        assert!(result.error.is_none(), "got error: {:?}", result.error);
        assert!(
            result.stdout.iter().any(|line| line == "2"),
            "expected '2' in stdout; got {:?}",
            result.stdout
        );
    }

    #[test]
    fn execute_print_captures_via_print_shim() {
        let mut host = AdminReplHost::new();
        let snapshot = WorldSnapshot::default();
        let code = match host.compile_or_incomplete("print('hello')\n") {
            CompileOutcome::Complete(c) => c,
            other => panic!("compile failed: {:?}", other.label()),
        };
        let result = host.execute_compiled(code, snapshot, None);
        assert!(result.error.is_none(), "got error: {:?}", result.error);
        assert!(
            result.stdout.iter().any(|line| line == "hello"),
            "expected 'hello' in stdout; got {:?}",
            result.stdout
        );
    }

    #[test]
    fn attach_player_sets_pending_attach() {
        let mut host = AdminReplHost::new();
        let snapshot = WorldSnapshot::default();
        let code = match host.compile_or_incomplete("world.attach_player(7)\n") {
            CompileOutcome::Complete(c) => c,
            other => panic!("compile failed: {:?}", other.label()),
        };
        let result = host.execute_compiled(code, snapshot, None);
        assert!(result.error.is_none(), "got error: {:?}", result.error);
        assert_eq!(result.attach, Some(Some(7)));
    }

    #[test]
    fn world_verbs_have_docstrings() {
        let mut host = AdminReplHost::new();
        // For each verb, evaluate its `__doc__`. The displayhook prints
        // `repr(value)` which for non-empty strings is `'<text>'` — so a
        // docstring shows up as a single non-empty quoted line.
        for verb in [
            "world.spawn",
            "world.give",
            "world.attach_player",
            "world.player",
            "world.help",
            "world.cast_spell",
        ] {
            let snapshot = WorldSnapshot::default();
            let src = format!("{verb}.__doc__\n");
            let code = match host.compile_or_incomplete(&src) {
                CompileOutcome::Complete(c) => c,
                other => panic!("compile of `{src}` failed: {:?}", other.label()),
            };
            let result = host.execute_compiled(code, snapshot, None);
            assert!(
                result.error.is_none(),
                "{verb}.__doc__ raised: {:?}",
                result.error
            );
            // The text signature header `($module, ...)\n--\n\n<doc>` is
            // generated by the `#[pyfunction]` macro and present whenever a
            // doc comment exists. A None / empty docstring would show up
            // as the bare repr `'None'` or the empty string `''`.
            let joined: String = result.stdout.join("\n");
            assert!(
                joined.contains("$module") && joined.contains("--"),
                "{verb}.__doc__ produced no docstring; got {joined:?}"
            );
        }
    }

    impl CompileOutcome {
        fn label(&self) -> &'static str {
            match self {
                CompileOutcome::Complete(_) => "Complete",
                CompileOutcome::Incomplete => "Incomplete",
                CompileOutcome::SyntaxError(_) => "SyntaxError",
            }
        }
    }
}

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
use rustpython_vm::compiler::Mode;
use rustpython_vm::scope::Scope;
use rustpython_vm::{Interpreter, PyObjectRef, VirtualMachine};

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

def _mud_display(value):
    # Echo a bare expression's value (REPL-style). None is swallowed so plain
    # statements stay quiet; collections pretty-print, scalars show their repr.
    if value is None:
        return
    if isinstance(value, (list, tuple, dict, set, frozenset)):
        world.log(pprint.pformat(value, width=120))
    else:
        world.log(repr(value))

# --- discoverability: help(x) and apropos("term") -----------------------
# These read live dir()/__doc__ so they never drift from the real API.

def _mud_first_doc(obj):
    doc = getattr(obj, "__doc__", None) or ""
    # The #[pyfunction] macro prefixes a "name(sig)\n--\n\n" text-signature
    # header; drop it so the summary is the human prose.
    if "\n--\n" in doc:
        doc = doc.split("\n--\n", 1)[1]
    for line in doc.splitlines():
        line = line.strip()
        if line:
            return line
    return ""

def _mud_public_members(obj):
    return [n for n in dir(obj) if not n.startswith("_")]

class _IdNamespace:
    """Tab-completable access to spawnable ids. Flat ids are direct attributes
    (world.types.health_potion -> 'health_potion'); module ids nest on their
    '/' separator (world.types.haunted_mill.moonshade_grain ->
    'haunted_mill/moonshade_grain'). For an id with characters that aren't
    valid attribute names, subscript by the full string: world.types['mod/id'].
    Pass the resolved string to world.spawn / world.cast_spell."""
    def __init__(self, lister, name, prefix=""):
        self._lister = lister     # () -> list[str] of FULL ids
        self._name = name         # "types" / "spells", for messages
        self._prefix = prefix     # "" at the root, "haunted_mill/" inside a module
    def _segments(self):
        # The next path segment after self._prefix for every id beneath it.
        out = []
        for full in self._lister():
            if full.startswith(self._prefix):
                out.append(full[len(self._prefix):].split("/", 1)[0])
        return out
    def __dir__(self):
        return sorted(set(self._segments()))
    def __getattr__(self, attr):
        ids = self._lister()
        full = self._prefix + attr
        if full in ids:
            return full                                  # exact leaf id
        if any(i.startswith(full + "/") for i in ids):
            return _IdNamespace(self._lister, self._name, full + "/")  # module branch
        raise AttributeError("world.%s has no id %r" % (self._name, full))
    def __getitem__(self, key):
        ids = self._lister()
        full = key if key in ids else self._prefix + key
        if full in ids:
            return full
        raise KeyError("world.%s has no id %r" % (self._name, key))
    def __repr__(self):
        return "<world.%s%s: %d entries — dir() or Tab>" % (
            self._name, "." + self._prefix.rstrip("/") if self._prefix else "",
            len(set(self._segments())))

# Attached to the `world` module so they're namespaced + discoverable.
world.types = _IdNamespace(world.object_types, "types")
world.spells = _IdNamespace(world.spell_ids, "spells")

def help(obj=None):
    """help() shows the world overview; help(world.spawn) a verb's full
    docstring; help(world.player()) dumps a Player's live fields + methods."""
    if obj is None or obj is world:
        world.help()
        return
    if callable(obj) and not isinstance(obj, type):
        world.log(getattr(obj, "__name__", repr(obj)))
        for line in (getattr(obj, "__doc__", None) or "(no docstring)").splitlines():
            world.log("  " + line)
        return
    # A class (e.g. Player) lists member names + one-line docs; an instance
    # dumps each property's live value (most useful when debugging a player).
    is_class = isinstance(obj, type)
    cls = obj if is_class else type(obj)
    world.log("%s — %s" % (getattr(cls, "__name__", repr(cls)), _mud_first_doc(cls)))
    for name in _mud_public_members(cls):
        attr = getattr(cls, name, None)
        if callable(attr):
            world.log("  %-22s %s" % (name + "()", _mud_first_doc(attr)))
        elif is_class:
            world.log("  %-22s %s" % (name, _mud_first_doc(attr)))
        else:
            try:
                world.log("  %-22s = %r" % (name, getattr(obj, name)))
            except Exception as exc:
                world.log("  %-22s = <%s>" % (name, type(exc).__name__))

def apropos(term):
    """Search world.* and Player members by name and docstring for `term`."""
    term = str(term).lower()
    hits = []
    def _scan(container, prefix):
        for name in _mud_public_members(container):
            member = getattr(container, name, None)
            haystack = (name + " " + (getattr(member, "__doc__", "") or "")).lower()
            if term in haystack:
                hits.append((prefix + name, _mud_first_doc(member)))
    _scan(world, "world.")
    Player = getattr(world, "Player", None)
    if Player is not None:
        _scan(Player, "Player.")
    if not hits:
        world.log("apropos(%r): no matches" % term)
        return
    for label, summary in sorted(hits):
        world.log("%-28s %s" % (label, summary))
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

impl Default for PythonConsoleHost {
    fn default() -> Self {
        Self::new()
    }
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

        let result: rustpython_vm::PyResult<()> = install_ctx(trait_ctx, || {
            self.interpreter.enter(|vm| {
                let scope = (*self.scope).clone();
                // IPython-style: a bare expression echoes its value through
                // `_mud_display`. Compile as an expression first; fall back to
                // a statement/exec compile for assignments, defs, loops, and
                // anything else that isn't a single expression.
                match vm.compile(command, Mode::Eval, "<mud-console>".to_owned()) {
                    Ok(code) => {
                        let value = vm.run_code_obj(code, scope.clone())?;
                        if !vm.is_none(&value) {
                            if let Ok(display) = scope.globals.get_item("_mud_display", vm) {
                                display.call((value,), vm)?;
                            }
                        }
                        Ok(())
                    }
                    Err(_) => {
                        let code = vm
                            .compile(command, Mode::Exec, "<mud-console>".to_owned())
                            .map_err(|err| vm.new_syntax_error(&err, Some(command)))?;
                        vm.run_code_obj(code, scope).map(drop)
                    }
                }
            })
        });

        let mut output = PythonExecOutput::default();

        if let Err(py_err) = result {
            // Render a real Python traceback (`NameError: name 'foo' is not
            // defined`, etc.) instead of the useless `Debug` of the exception
            // ref. `write_exception` needs the VM, so re-enter to format.
            let traceback = self.interpreter.enter(|vm| {
                let mut buf = String::new();
                vm.write_exception(&mut buf, &py_err).ok();
                buf
            });
            for line in traceback.trim_end().lines() {
                output.lines.push((line.to_owned(), LineStyle::Traceback));
            }
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

    /// Candidate identifiers to substitute for the trailing token of
    /// `text_before_cursor`, powering Tab completion:
    ///
    /// - bare `wor` → globals + builtins starting with `wor`;
    /// - dotted `world.sp` → members of `world` starting with `sp`.
    ///
    /// Always returns bare member names (`spawn`, not `world.spawn`) so the
    /// caller replaces only the trailing token. Only a *pure dotted-name*
    /// base is evaluated — a base ending in a call/subscript (`world.player()`)
    /// is refused, so completion never executes a side-effecting expression.
    ///
    /// `snapshot` is installed as the API context for the duration so that
    /// snapshot-backed introspection resolves — most importantly
    /// `dir(world.types)` / `dir(world.spells)`, which enumerate the spawnable
    /// id strings.
    pub fn complete_at(&self, text_before_cursor: &str, snapshot: WorldSnapshot) -> Vec<String> {
        let attr_prefix = trailing_identifier(text_before_cursor).to_owned();
        let head = &text_before_cursor[..text_before_cursor.len() - attr_prefix.len()];
        // `Some(base)` ⇒ dotted access; `base` is `None` when the thing
        // before the dot isn't safe to evaluate. `None` ⇒ bare identifier.
        let base: Option<Option<String>> = head.strip_suffix('.').map(evaluable_base);

        let ctx: Arc<dyn ApiContext> = Arc::new(AdminApiContext::new(snapshot));
        install_ctx(ctx, || {
            self.interpreter.enter(|vm| {
                let mut names: Vec<String> = match base {
                    Some(Some(base_src)) => {
                        let Ok(code) = vm.compile(&base_src, Mode::Eval, "<complete>".to_owned())
                        else {
                            return Vec::new();
                        };
                        match vm.run_code_obj(code, (*self.scope).clone()) {
                            Ok(obj) => dir_names(vm, &obj),
                            Err(_) => return Vec::new(),
                        }
                    }
                    // Dotted, but the base ends in a call/subscript/operator —
                    // refuse rather than risk evaluating it.
                    Some(None) => return Vec::new(),
                    None => {
                        // A bare empty prefix is a Tab on whitespace; don't dump
                        // the whole namespace.
                        if attr_prefix.is_empty() {
                            return Vec::new();
                        }
                        let mut names = global_names(vm, &self.scope);
                        let builtins: PyObjectRef = vm.builtins.clone().into();
                        names.extend(dir_names(vm, &builtins));
                        names
                    }
                };
                names.retain(|s| s.starts_with(&attr_prefix));
                names.sort();
                names.dedup();
                names
            })
        })
    }
}

/// Trailing identifier-ish token of `input`: scans right-to-left over
/// `[A-Za-z0-9_]`. Mirrors the handler's own scan so replacement length and
/// the completed prefix agree.
fn trailing_identifier(input: &str) -> &str {
    let bytes = input.as_bytes();
    let mut split = bytes.len();
    for (i, b) in bytes.iter().enumerate().rev() {
        if b.is_ascii_alphanumeric() || *b == b'_' {
            split = i;
        } else {
            break;
        }
    }
    &input[split..]
}

/// If `s` ends with a pure dotted-name (`foo`, `a.b.c`) — identifiers joined
/// by dots, no calls/subscripts/operators — return that trailing expression,
/// else `None`. Gates which completion bases are safe to evaluate.
fn evaluable_base(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut start = bytes.len();
    for (i, b) in bytes.iter().enumerate().rev() {
        if b.is_ascii_alphanumeric() || *b == b'_' || *b == b'.' {
            start = i;
        } else {
            break;
        }
    }
    let base = &s[start..];
    if base.is_empty() || base.starts_with('.') || base.ends_with('.') {
        return None;
    }
    // Every dot-separated segment must be a valid identifier (non-empty, not
    // starting with a digit).
    if base
        .split('.')
        .any(|seg| seg.is_empty() || seg.as_bytes()[0].is_ascii_digit())
    {
        return None;
    }
    Some(base.to_owned())
}

/// Public member names of `obj` via Python's `dir()`.
fn dir_names(vm: &VirtualMachine, obj: &PyObjectRef) -> Vec<String> {
    match vm.dir(Some(obj.clone())) {
        Ok(list) => list
            .borrow_vec()
            .iter()
            .filter_map(|item| item.str(vm).ok().map(|s| s.as_str().to_owned()))
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Names bound in the persistent scope's globals.
fn global_names(vm: &VirtualMachine, scope: &Scope) -> Vec<String> {
    let globals = scope.globals.clone();
    (&*globals)
        .into_iter()
        .filter_map(|(key, _value)| key.str(vm).ok().map(|s| s.as_str().to_owned()))
        .collect()
}

impl Drop for PythonConsoleHost {
    fn drop(&mut self) {
        // RustPython teardown currently hangs or crashes on application
        // shutdown. Intentionally leaking the VM state is acceptable here
        // because the process is already exiting and the OS will reclaim
        // the memory.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_identifier_scans_last_token() {
        assert_eq!(trailing_identifier("world.sp"), "sp");
        assert_eq!(trailing_identifier("wor"), "wor");
        assert_eq!(trailing_identifier("world."), "");
        assert_eq!(trailing_identifier(""), "");
    }

    #[test]
    fn evaluable_base_accepts_dotted_names_only() {
        assert_eq!(evaluable_base("world").as_deref(), Some("world"));
        assert_eq!(evaluable_base("x = world").as_deref(), Some("world"));
        assert_eq!(evaluable_base("a.b.c").as_deref(), Some("a.b.c"));
        // calls / subscripts / empties / malformed dotted names are refused
        assert_eq!(evaluable_base("world.player()"), None);
        assert_eq!(evaluable_base("xs[0]"), None);
        assert_eq!(evaluable_base(""), None);
        assert_eq!(evaluable_base("a..b"), None);
    }

    #[test]
    fn complete_dotted_world_members() {
        let host = PythonConsoleHost::new();
        let matches = host.complete_at("world.sp", WorldSnapshot::default());
        assert!(matches.iter().any(|m| m == "spawn"), "got {matches:?}");
        assert!(
            matches.iter().all(|m| m.starts_with("sp")),
            "got {matches:?}"
        );
    }

    #[test]
    fn complete_dotted_empty_lists_all_members() {
        let host = PythonConsoleHost::new();
        let matches = host.complete_at("world.", WorldSnapshot::default());
        assert!(matches.iter().any(|m| m == "spawn"), "got {matches:?}");
        assert!(matches.iter().any(|m| m == "help"), "got {matches:?}");
    }

    #[test]
    fn complete_bare_prefix_includes_globals() {
        let host = PythonConsoleHost::new();
        let matches = host.complete_at("wor", WorldSnapshot::default());
        assert!(matches.iter().any(|m| m == "world"), "got {matches:?}");
    }

    #[test]
    fn complete_refuses_call_base_and_unknown_name() {
        let host = PythonConsoleHost::new();
        assert!(host
            .complete_at("world.player().fo", WorldSnapshot::default())
            .is_empty());
        assert!(host
            .complete_at("nonexistent_obj.fo", WorldSnapshot::default())
            .is_empty());
    }

    #[test]
    fn complete_bare_empty_returns_nothing() {
        let host = PythonConsoleHost::new();
        assert!(host.complete_at("   ", WorldSnapshot::default()).is_empty());
    }

    #[test]
    fn complete_spawn_ids_via_types_namespace() {
        let host = PythonConsoleHost::new();
        let snapshot = WorldSnapshot {
            object_types: vec![
                "health_potion".to_owned(),
                "healing_herb".to_owned(),
                "bronze_sword".to_owned(),
            ],
            ..Default::default()
        };
        let matches = host.complete_at("world.types.heal", snapshot);
        assert!(
            matches.iter().any(|m| m == "health_potion"),
            "got {matches:?}"
        );
        assert!(
            matches.iter().all(|m| m.starts_with("heal")),
            "expected only heal* ids, got {matches:?}"
        );
        assert!(
            !matches.iter().any(|m| m == "bronze_sword"),
            "should be prefix-filtered, got {matches:?}"
        );
    }

    #[test]
    fn types_namespace_resolves_to_id_string() {
        let mut host = PythonConsoleHost::new();
        let snapshot = WorldSnapshot {
            object_types: vec!["health_potion".to_owned()],
            ..Default::default()
        };
        let out = host.execute("world.types.health_potion", snapshot);
        let text: String = out
            .lines
            .iter()
            .map(|(l, _)| l.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("'health_potion'"),
            "expected id string, got {text:?}"
        );
    }

    #[test]
    fn types_namespace_nests_module_ids_on_slash() {
        let host = PythonConsoleHost::new();
        let snapshot = WorldSnapshot {
            object_types: vec![
                "health_potion".to_owned(),
                "haunted_mill/moonshade_grain".to_owned(),
                "haunted_mill/rusty_gear".to_owned(),
            ],
            ..Default::default()
        };
        // Top level offers the module as a single branch (its '/' ids collapse
        // to one segment), never the slashed id itself.
        let top = host.complete_at("world.types.haun", snapshot.clone());
        assert_eq!(top, vec!["haunted_mill".to_owned()], "got {top:?}");

        // Descending into the module completes its leaf ids.
        let nested = host.complete_at("world.types.haunted_mill.moon", snapshot);
        assert!(
            nested.iter().any(|m| m == "moonshade_grain"),
            "got {nested:?}"
        );
        assert!(
            nested.iter().all(|m| m.starts_with("moon")),
            "got {nested:?}"
        );
    }

    #[test]
    fn types_namespace_resolves_module_id_to_full_slash_string() {
        let mut host = PythonConsoleHost::new();
        let snapshot = WorldSnapshot {
            object_types: vec!["haunted_mill/moonshade_grain".to_owned()],
            ..Default::default()
        };
        let out = host.execute("world.types.haunted_mill.moonshade_grain", snapshot);
        let text: String = out
            .lines
            .iter()
            .map(|(l, _)| l.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("'haunted_mill/moonshade_grain'"),
            "expected full slashed id, got {text:?}"
        );
    }

    #[test]
    fn types_namespace_subscript_resolves_any_full_id() {
        let mut host = PythonConsoleHost::new();
        let snapshot = WorldSnapshot {
            object_types: vec!["haunted_mill/moonshade_grain".to_owned()],
            ..Default::default()
        };
        let out = host.execute("world.types['haunted_mill/moonshade_grain']", snapshot);
        let text: String = out
            .lines
            .iter()
            .map(|(l, _)| l.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("'haunted_mill/moonshade_grain'"),
            "got {text:?}"
        );
    }

    #[test]
    fn types_namespace_unknown_id_raises_attribute_error() {
        let mut host = PythonConsoleHost::new();
        let snapshot = WorldSnapshot {
            object_types: vec!["health_potion".to_owned()],
            ..Default::default()
        };
        let out = host.execute("world.types.no_such_thing", snapshot);
        let text: String = out
            .lines
            .iter()
            .map(|(l, _)| l.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("AttributeError"),
            "expected AttributeError, got {text:?}"
        );
    }

    /// Join a run's output lines, asserting nothing raised.
    fn run_output(host: &mut PythonConsoleHost, src: &str) -> String {
        let out = host.execute(src, WorldSnapshot::default());
        assert!(
            !out.lines
                .iter()
                .any(|(_, s)| matches!(s, LineStyle::Traceback)),
            "`{src}` raised: {:?}",
            out.lines
        );
        out.lines
            .iter()
            .map(|(l, _)| l.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn world_help_overview_lists_verbs_dynamically() {
        let mut host = PythonConsoleHost::new();
        let text = run_output(&mut host, "world.help()");
        assert!(text.contains("world API"), "missing header: {text}");
        assert!(text.contains("spawn"), "missing spawn verb: {text}");
        assert!(text.contains("Read"), "missing Read category: {text}");
    }

    #[test]
    fn help_on_verb_shows_docstring() {
        let mut host = PythonConsoleHost::new();
        let text = run_output(&mut host, "help(world.spawn)");
        assert!(text.contains("spawn"), "{text}");
        assert!(
            text.contains("type_id"),
            "expected signature/doc text: {text}"
        );
    }

    #[test]
    fn apropos_searches_world_and_player_members() {
        let mut host = PythonConsoleHost::new();
        let text = run_output(&mut host, "apropos('skill')");
        // `set_skill` / `grant_skill_points` live on Player; apropos scans both.
        assert!(text.contains("skill"), "no skill matches: {text}");
        assert!(
            text.contains("Player."),
            "expected a Player.* match: {text}"
        );
    }

    #[test]
    fn help_on_player_class_lists_members() {
        let mut host = PythonConsoleHost::new();
        let text = run_output(&mut host, "help(world.Player)");
        assert!(text.contains("Player"), "{text}");
        assert!(text.contains("grant_xp"), "missing method: {text}");
        assert!(text.contains("level"), "missing property: {text}");
    }

    fn snapshot_with_one_player(id: u64, name: &str) -> WorldSnapshot {
        use crate::scripting_api::snapshots::{AttributeMap, PlayerView, VitalsView};
        let mut snapshot = WorldSnapshot::default();
        snapshot.players.push(PlayerView {
            player_id: id,
            object_id: Some(100 + id),
            space_id: 1,
            x: 5,
            y: 7,
            z: 0,
            vitals: VitalsView {
                health: 42.0,
                max_health: 100.0,
                mana: 10.0,
                max_mana: 40.0,
            },
            facing: "South".to_owned(),
            display_name: name.to_owned(),
            class_label: "Fighter".to_owned(),
            level: 3,
            current_xp: 0,
            xp_into_level: 0,
            xp_for_next: Some(1000),
            attributes: AttributeMap {
                strength: 10,
                agility: 12,
                constitution: 11,
                willpower: 10,
                charisma: 10,
                focus: 10,
            },
            skill_ranks: vec![("Thievery".to_owned(), 0)],
            available_skill_points: 0,
        });
        snapshot
    }

    #[test]
    fn help_on_player_instance_dumps_live_values() {
        let mut host = PythonConsoleHost::new();
        let snapshot = snapshot_with_one_player(1, "Alice");
        let out = host.execute("help(world.find_player(1))", snapshot);
        let text: String = out
            .lines
            .iter()
            .map(|(l, _)| l.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !out.lines
                .iter()
                .any(|(_, s)| matches!(s, LineStyle::Traceback)),
            "raised: {text}"
        );
        // Live property values, not docstrings.
        assert!(text.contains("name") && text.contains("Alice"), "{text}");
        assert!(text.contains("level") && text.contains("3"), "{text}");
        // Methods still rendered with a trailing ().
        assert!(text.contains("grant_xp()"), "missing method: {text}");
    }

    #[test]
    fn bare_expression_echoes_its_value() {
        let mut host = PythonConsoleHost::new();
        let text = run_output(&mut host, "1 + 1");
        assert!(text.lines().any(|l| l == "2"), "expected '2', got {text:?}");
    }

    #[test]
    fn string_expression_echoes_repr_not_str() {
        let mut host = PythonConsoleHost::new();
        let text = run_output(&mut host, "'hi'");
        assert!(text.contains("'hi'"), "expected repr quotes, got {text:?}");
    }

    #[test]
    fn statement_is_silent() {
        let mut host = PythonConsoleHost::new();
        let out = host.execute("x = 5", WorldSnapshot::default());
        assert!(
            out.lines.is_empty(),
            "assignment should produce no output, got {:?}",
            out.lines
        );
    }

    #[test]
    fn assignment_then_expression_echoes() {
        let mut host = PythonConsoleHost::new();
        host.execute("y = 41", WorldSnapshot::default());
        let text = run_output(&mut host, "y + 1");
        assert!(text.lines().any(|l| l == "42"), "got {text:?}");
    }

    #[test]
    fn player_attribute_mapping_is_read_only() {
        let mut host = PythonConsoleHost::new();
        let snapshot = snapshot_with_one_player(1, "Alice");
        let out = host.execute("world.find_player(1).attributes['strength'] = 99", snapshot);
        // mappingproxy rejects assignment — surfaced as a traceback line.
        assert!(
            out.lines
                .iter()
                .any(|(_, s)| matches!(s, LineStyle::Traceback)),
            "expected a read-only error, got {:?}",
            out.lines
        );
    }

    #[test]
    fn name_error_shows_real_traceback() {
        let mut host = PythonConsoleHost::new();
        let out = host.execute("nonexistent_variable_xyz", WorldSnapshot::default());
        let text: String = out
            .lines
            .iter()
            .map(|(l, _)| l.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            out.lines
                .iter()
                .any(|(_, s)| matches!(s, LineStyle::Traceback)),
            "expected traceback styling, got {:?}",
            out.lines
        );
        assert!(
            text.contains("NameError"),
            "expected NameError, got {text:?}"
        );
        assert!(
            text.contains("nonexistent_variable_xyz"),
            "expected the offending name, got {text:?}"
        );
        // The old useless Debug repr must be gone.
        assert!(
            !text.contains("PyRef") && !text.contains("Python error:"),
            "leaked debug repr: {text:?}"
        );
    }

    #[test]
    fn syntax_error_shows_real_message() {
        let mut host = PythonConsoleHost::new();
        let out = host.execute(")(", WorldSnapshot::default());
        let text: String = out
            .lines
            .iter()
            .map(|(l, _)| l.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("SyntaxError"),
            "expected SyntaxError, got {text:?}"
        );
    }

    #[test]
    fn player_attribute_mapping_still_reads() {
        let mut host = PythonConsoleHost::new();
        let snapshot = snapshot_with_one_player(1, "Alice");
        let text = {
            let out = host.execute("world.find_player(1).attributes['strength']", snapshot);
            out.lines
                .iter()
                .map(|(l, _)| l.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert!(
            text.lines().any(|l| l == "10"),
            "expected '10', got {text:?}"
        );
    }
}

//! Shared API surface for all embedded RustPython VMs in the project.
//!
//! Both the admin console (`src/scripting/`) and the quest engine
//! (`src/quest/`) register the same `world` Python module by routing
//! pyfunction calls through an `ApiContext` trait object. Each VM installs
//! its own context (admin vs. quest) for the duration of a Python call via
//! [`install_ctx`]; the pyfunctions in [`bindings`] consult that context
//! through [`with_ctx`].
//!
//! Why a single shared module: keeps the read/write surface in one place,
//! gives quest scripts the same world-introspection vocabulary the admin
//! console has, and gives the admin console the same inventory/var
//! manipulation vocabulary quest hooks have. Capability differences
//! (admin-only verbs like `teleport` / `set_vitals`) are enforced inside
//! each `ApiContext` impl.

use std::cell::RefCell;
use std::sync::Arc;

use crate::dialog::variable_storage::YarnValueDump;
use crate::game::commands::GameCommand;

pub mod bindings;
pub mod build;
pub mod snapshots;

pub use snapshots::{
    FloorMapView, PlayerView, SpaceView, VitalsView, WorldObjectView, WorldSnapshot,
};

/// Errors a context impl can return when an API call is rejected. The
/// pymodule converts these into RustPython exceptions.
#[derive(Clone, Debug)]
pub enum ApiError {
    /// The call is not allowed in this context (e.g. `teleport` from a
    /// quest hook, or `set_var` from the admin console).
    NotPermitted(&'static str),
    /// The call's arguments were invalid.
    Invalid(String),
}

impl ApiError {
    pub fn as_string(&self) -> String {
        match self {
            ApiError::NotPermitted(msg) => format!("not permitted: {msg}"),
            ApiError::Invalid(msg) => format!("invalid: {msg}"),
        }
    }
}

/// Capability surface shared by every Python VM. The admin console and
/// the quest engine each implement this trait differently — admin gets
/// every verb; quest contexts reject admin-only verbs.
pub trait ApiContext: Send + Sync {
    fn is_admin(&self) -> bool;

    /// The player whose actions this context represents. For the admin
    /// console this is the local player (or `None` if no player has
    /// joined yet); for quest hooks this is the quest's active player.
    fn caller_player_id(&self) -> Option<u64>;

    fn snapshot(&self) -> &WorldSnapshot;

    /// Append a free-form log line to the context's output channel
    /// (admin console output buffer, or quest engine `info!` log).
    fn log(&self, message: String);

    /// Queue a `GameCommand` for the server to process next tick. Returns
    /// `Err` when the command isn't allowed in this context.
    fn queue_command(&self, command: GameCommand) -> Result<(), ApiError>;

    /// Yarn-variable read for quest hooks. Default impl rejects with
    /// `NotPermitted`; the quest context overrides it.
    fn get_yarn_var(&self, _name: &str) -> Result<Option<YarnValueDump>, ApiError> {
        Err(ApiError::NotPermitted(
            "get_var requires a quest hook context",
        ))
    }

    /// Yarn-variable write — quest only.
    fn set_yarn_var(&self, _name: &str, _value: YarnValueDump) -> Result<(), ApiError> {
        Err(ApiError::NotPermitted(
            "set_var requires a quest hook context",
        ))
    }

    /// Mark a quest finished. `failed = true` ⇔ `fail_quest`. Quest only.
    fn end_quest(&self, _quest_id: &str, _failed: bool) -> Result<(), ApiError> {
        Err(ApiError::NotPermitted(
            "complete_quest / fail_quest require a quest hook context",
        ))
    }

    /// Caller's inventory count for `type_id`. Quest contexts return the
    /// snapshot they were initialised with; admin contexts can return 0
    /// when the count is not available.
    fn caller_inventory_count(&self, type_id: &str) -> u32 {
        self.snapshot()
            .caller_inventory
            .get(type_id)
            .copied()
            .unwrap_or(0)
    }

    /// Admin-only escape hatch — clears the persistent Python scope so
    /// the next input starts fresh. Default impl rejects.
    fn reset_scope(&self) -> Result<(), ApiError> {
        Err(ApiError::NotPermitted("reset() is admin-only"))
    }

    /// Admin-REPL only: bind the *session*'s caller to a specific live
    /// player so verbs like `give` / `teleport` / `player()` behave as if
    /// that account were issuing them. Pass `None` to detach. Default
    /// impl rejects — the in-game console (which already has a local
    /// player) and quest hooks (which have a fixed quest player) don't
    /// support changing caller mid-call.
    fn attach_player(&self, _player_id: Option<u64>) -> Result<(), ApiError> {
        Err(ApiError::NotPermitted(
            "attach_player is only available in the admin REPL",
        ))
    }
}

thread_local! {
    static WORLD_API_CTX: RefCell<Option<Arc<dyn ApiContext>>> = const { RefCell::new(None) };
}

/// Install `ctx` into the thread-local for the duration of `f`. Both VMs
/// wrap each `Interpreter::enter` block with this so the `world`
/// pymodule's pyfunctions can find their context.
///
/// The previous context (if any) is restored when this returns; nesting
/// is therefore safe in principle, but neither current caller nests.
pub fn install_ctx<R>(ctx: Arc<dyn ApiContext>, f: impl FnOnce() -> R) -> R {
    let previous = WORLD_API_CTX.with(|cell| cell.replace(Some(ctx)));
    struct RestoreOnDrop(Option<Arc<dyn ApiContext>>);
    impl Drop for RestoreOnDrop {
        fn drop(&mut self) {
            let prev = self.0.take();
            WORLD_API_CTX.with(|cell| {
                *cell.borrow_mut() = prev;
            });
        }
    }
    let _guard = RestoreOnDrop(previous);
    f()
}

/// Look up the currently-installed context. `None` when called outside
/// an `install_ctx` block (which would be a programming error — pymodule
/// pyfunctions only run while a context is installed).
pub fn with_ctx<R>(f: impl FnOnce(&dyn ApiContext) -> R) -> Option<R> {
    WORLD_API_CTX.with(|cell| cell.borrow().as_ref().map(|ctx| f(&**ctx)))
}

/// Convenience for pyfunctions: run `f` against the current context, or
/// return `default` if no context is installed.
pub fn with_ctx_or<R>(default: R, f: impl FnOnce(&dyn ApiContext) -> R) -> R {
    with_ctx(f).unwrap_or(default)
}

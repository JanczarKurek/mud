//! Ungated `SystemSet`s ordering the per-frame network pipeline. They must
//! stay feature-ungated so client-side systems and `server-sim`-gated systems
//! can order against each other across the feature boundary without
//! `.before(fn)` references that silently drop when the target isn't
//! registered (same precedent as `PythonConsoleToggleSet`).
//!
//! Frame order (sets are no-ops in modes where their systems don't exist):
//!
//! ```text
//! input systems
//!   → NetClientSend      (client: serialize commands → transport)
//!   → NetServerReceive   (server: transport → PendingGameCommands; before CommandIntercept)
//!   → simulation         (CommandIntercept → process_game_commands → npc/combat/…)
//!   → NetServerSend      (server: per-peer projection diff → transport)
//!   → NetClientReceive   (client: transport → PendingGameEvents/UiEvents)
//!   → apply_game_events_to_client_state → presentation
//! ```
//!
//! With a loopback transport in a single App (EmbeddedClient), this chain
//! makes a command reflect in `ClientGameState` within the same frame.

use bevy::prelude::SystemSet;

/// Client → transport: `flush_client_commands_to_server`.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, SystemSet)]
pub struct NetClientSend;

/// Transport → server command queue: `poll_tcp_server_messages`.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, SystemSet)]
pub struct NetServerReceive;

/// Server → transport: `flush_server_messages` (per-peer projection diff).
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, SystemSet)]
pub struct NetServerSend;

/// Transport → client event queues: `poll_tcp_client_messages`.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, SystemSet)]
pub struct NetClientReceive;

//! Server-side, admin-gated Python REPL over the game protocol.
//!
//! The in-game console UI (see `scripting::systems`) is a thin client: each
//! submitted line travels as `GameCommand::AdminExec`, is executed here
//! against the shared [`AdminReplHost`], and the captured output returns as
//! `GameUiEvent::ReplOutput`. Multi-line blocks buffer per-player, mirroring
//! the UNIX-socket admin REPL (`network::admin`) — and both front ends share
//! one interpreter scope, so admins can collaborate on globals regardless of
//! how they connected.
//!
//! Privilege model: a command is honored when the issuing peer's account
//! carries the `is_admin` flag (the embedded loopback peer always does), or
//! when the command is untargeted — untargeted entries in
//! `PendingGameCommands` can only originate server-side (scripts, the socket
//! REPL); client intent always arrives peer-tagged via the wire.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::app::state::simulation_active;
use crate::game::commands::GameCommand;
use crate::game::resources::{GameUiEvent, PendingGameCommands, PendingGameUiEvents};
use crate::network::resources::TcpServerState;
use crate::player::components::PlayerId;
use crate::scripting::admin_host::{AdminReplHost, CompileOutcome};
use crate::scripting_api::build::WorldSnapshotParams;

/// Per-player REPL session state. Mirrors `network::admin::AdminSession`
/// minus the socket: pending multi-line input plus the `attach_player`
/// caller override.
#[derive(Default)]
struct GameReplSession {
    pending_input: String,
    /// `Some` after `world.attach_player(id)` — subsequent commands act as
    /// that player instead of the issuer. `None` = act as the issuer.
    caller_override: Option<PlayerId>,
}

#[derive(Resource, Default)]
pub struct GameReplSessions {
    by_player: HashMap<PlayerId, GameReplSession>,
}

/// Server plugin for the wire-facing REPL. Registered in EmbeddedClient and
/// HeadlessServer modes; shares the `AdminReplHost` NonSend with
/// `AdminReplPlugin` when both are present (insert-if-absent on both sides).
pub struct GameReplPlugin;

impl Plugin for GameReplPlugin {
    fn build(&self, app: &mut App) {
        if app
            .world()
            .get_non_send_resource::<AdminReplHost>()
            .is_none()
        {
            app.insert_non_send_resource(AdminReplHost::new());
        }
        app.init_resource::<GameReplSessions>().add_systems(
            Update,
            (process_admin_exec_commands, process_admin_account_commands)
                .in_set(crate::game::CommandIntercept)
                .run_if(simulation_active),
        );
    }
}

/// Is the player behind `player_id` allowed to use admin commands?
/// `None` (untargeted) is trusted — see the module docs.
fn issuer_is_admin(server_state: Option<&TcpServerState>, player_id: Option<PlayerId>) -> bool {
    match player_id {
        None => true,
        Some(id) => server_state.is_some_and(|state| {
            state
                .peers
                .values()
                .any(|peer| peer.player_id == Some(id) && peer.is_admin)
        }),
    }
}

fn push_output(
    ui_events: &mut PendingGameUiEvents,
    issuer: Option<PlayerId>,
    lines: Vec<String>,
    error: Option<String>,
    incomplete: bool,
) {
    let Some(issuer) = issuer else {
        // Untargeted AdminExec has no peer to answer; log instead.
        for line in &lines {
            info!("game repl: {line}");
        }
        if let Some(error) = &error {
            warn!("game repl: {error}");
        }
        return;
    };
    ui_events.push(
        issuer,
        GameUiEvent::ReplOutput {
            lines,
            error,
            incomplete,
        },
    );
}

/// Drains `AdminExec` / `AdminReplReset` in `CommandIntercept`, executes on
/// the shared host, and answers via `ReplOutput` UI events.
pub fn process_admin_exec_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    mut ui_events: ResMut<PendingGameUiEvents>,
    mut sessions: ResMut<GameReplSessions>,
    mut host: NonSendMut<AdminReplHost>,
    snapshot_params: WorldSnapshotParams,
    server_state: Option<Res<TcpServerState>>,
) {
    enum ReplRequest {
        Exec(String),
        Reset,
    }

    let requests = pending_commands.drain_matching(|command| match command {
        GameCommand::AdminExec { code } => Ok(ReplRequest::Exec(code)),
        GameCommand::AdminReplReset => Ok(ReplRequest::Reset),
        other => Err(other),
    });
    if requests.is_empty() {
        return;
    }

    // Re-queued world.* commands land back into this queue after the drain,
    // so they're processed by the normal drainers later this same frame.
    let mut requeue: Vec<(Option<PlayerId>, GameCommand)> = Vec::new();

    for (issuer, request) in requests {
        if !issuer_is_admin(server_state.as_deref(), issuer) {
            warn!("game repl: non-admin {issuer:?} sent an admin REPL command; rejecting");
            push_output(
                &mut ui_events,
                issuer,
                Vec::new(),
                Some("not authorized: this account has no admin flag".to_owned()),
                false,
            );
            continue;
        }

        match request {
            ReplRequest::Reset => {
                host.reset_scope();
                if let Some(issuer) = issuer {
                    sessions.by_player.remove(&issuer);
                }
                push_output(
                    &mut ui_events,
                    issuer,
                    vec!["[System] interpreter restarted.".to_owned()],
                    None,
                    false,
                );
            }
            ReplRequest::Exec(code) => {
                // Sessions are keyed by issuer. Untargeted execs (server-side
                // producers) get a throwaway session — no multi-line buffering.
                let mut scratch = GameReplSession::default();
                let session: &mut GameReplSession = match issuer {
                    Some(id) => sessions.by_player.entry(id).or_default(),
                    None => &mut scratch,
                };

                // Blank line with nothing buffered: no-op prompt refresh.
                if code.trim().is_empty() && session.pending_input.is_empty() {
                    push_output(&mut ui_events, issuer, Vec::new(), None, false);
                    continue;
                }
                session.pending_input.push_str(&code);
                session.pending_input.push('\n');

                match host.compile_or_incomplete(&session.pending_input) {
                    CompileOutcome::Incomplete => {
                        push_output(&mut ui_events, issuer, Vec::new(), None, true);
                    }
                    CompileOutcome::SyntaxError(msg) => {
                        session.pending_input.clear();
                        push_output(&mut ui_events, issuer, Vec::new(), Some(msg), false);
                    }
                    CompileOutcome::Complete(compiled) => {
                        session.pending_input.clear();
                        let caller = session.caller_override.or(issuer);
                        let snapshot = snapshot_params.build_for_player(caller);
                        let result = host.execute_compiled(compiled, snapshot, caller.map(|p| p.0));

                        for cmd in result.queued_commands {
                            requeue.push((caller, cmd));
                        }
                        for (target, cmd) in result.targeted_commands {
                            requeue.push((Some(target), cmd));
                        }
                        if let Some(attach) = result.attach {
                            session.caller_override = attach.map(PlayerId);
                        }
                        push_output(&mut ui_events, issuer, result.stdout, result.error, false);
                    }
                }
            }
        }
    }

    for (target, cmd) in requeue {
        match target {
            Some(id) => pending_commands.push_for_player(id, cmd),
            None => pending_commands.push(cmd),
        }
    }
}

/// Drains `AdminSetAccountAdmin` — the `world.grant_admin` / `revoke_admin`
/// path. Same privilege rule as the REPL itself.
pub fn process_admin_account_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    mut ui_events: ResMut<PendingGameUiEvents>,
    server_state: Option<Res<TcpServerState>>,
    db: Option<Res<crate::accounts::AccountDbHandle>>,
) {
    for (issuer, (username, admin)) in pending_commands.drain_matching(|command| match command {
        GameCommand::AdminSetAccountAdmin { username, admin } => Ok((username, admin)),
        other => Err(other),
    }) {
        if !issuer_is_admin(server_state.as_deref(), issuer) {
            warn!("game repl: non-admin {issuer:?} tried to change the admin flag; rejecting");
            continue;
        }
        let Some(db) = db.as_deref() else {
            continue;
        };
        let result = db.lock().set_account_admin(&username, admin);
        let message = match result {
            Ok(()) => format!(
                "account '{username}' admin flag {}",
                if admin { "granted" } else { "revoked" }
            ),
            Err(err) => format!("failed to update admin flag for '{username}': {err}"),
        };
        info!("game repl: {message}");
        push_output(&mut ui_events, issuer, vec![message], None, false);
    }
}

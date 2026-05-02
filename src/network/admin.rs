//! Admin Python REPL listener — UNIX-domain-socket only.
//!
//! Wired into `HeadlessServer` mode when the operator passes
//! `--admin-socket [PATH]`. Auth is by filesystem permissions (default mode
//! `0600`, owner-only). One Python `Mode::Single` REPL on the Bevy main
//! thread, multiple concurrent socket sessions sharing one persistent
//! interpreter scope.
//!
//! This module is `#[cfg(unix)]` — on non-Unix platforms the plugin is a
//! no-op, surfacing a single warning at startup so the operator knows the
//! flag was ignored.

#![cfg(unix)]

use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use bevy::app::AppExit;
use bevy::ecs::message::MessageReader;
use bevy::log::{error, info, warn};
use bevy::prelude::*;

use crate::app::state::simulation_active;
use crate::game::resources::PendingGameCommands;
use crate::game::systems::process_game_commands;
use crate::player::components::PlayerId;
use crate::scripting::admin_host::{AdminReplHost, CompileOutcome};
use crate::scripting_api::build::WorldSnapshotParams;

const BANNER: &str = "mud2 admin REPL — Python on the live world. Ctrl-D to exit.";
const PROMPT_NEW: &str = ">>> ";
const PROMPT_CONT: &str = "... ";
/// Hard cap on the per-session line buffer. A peer that flooded us with
/// data and never sent `\n` would otherwise grow this unbounded.
const READ_BUFFER_LIMIT: usize = 65_536;

#[derive(Clone, Debug)]
pub struct AdminListenArgs {
    pub socket_path: PathBuf,
    /// Octal mode applied to the socket file after bind. `0o600` (owner
    /// rw) is the default; loosen to `0o660` if you've put admins in a
    /// shared group.
    pub mode: u32,
}

#[derive(Resource, Clone)]
pub struct AdminListenerConfig {
    pub socket_path: PathBuf,
    pub mode: u32,
}

#[derive(Resource, Default)]
pub struct AdminListenerState {
    listener: Option<UnixListener>,
    next_session_id: u64,
    sessions: HashMap<u64, AdminSession>,
}

struct AdminSession {
    stream: UnixStream,
    read_buffer: Vec<u8>,
    pending_input: String,
    caller: Option<PlayerId>,
}

pub struct AdminReplPlugin {
    pub args: AdminListenArgs,
}

impl Plugin for AdminReplPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(AdminListenerConfig {
            socket_path: self.args.socket_path.clone(),
            mode: self.args.mode,
        })
        .insert_resource(AdminListenerState::default())
        .insert_non_send_resource(AdminReplHost::new())
        .add_systems(Startup, start_admin_listener)
        .add_systems(
            Update,
            (
                accept_admin_connections.run_if(simulation_active),
                poll_admin_sessions
                    .before(process_game_commands)
                    .run_if(simulation_active),
            )
                .chain(),
        )
        .add_systems(Last, unlink_admin_socket_on_exit);
    }
}

fn start_admin_listener(config: Res<AdminListenerConfig>, mut state: ResMut<AdminListenerState>) {
    if state.listener.is_some() {
        return;
    }

    if let Err(err) = clear_stale_socket(&config.socket_path) {
        error!(
            "admin REPL: refusing to bind {} ({err}); cowardly leaving the existing file alone",
            config.socket_path.display()
        );
        return;
    }

    let listener = match UnixListener::bind(&config.socket_path) {
        Ok(l) => l,
        Err(err) => {
            error!(
                "admin REPL: failed to bind {}: {err}",
                config.socket_path.display()
            );
            return;
        }
    };

    if let Err(err) = listener.set_nonblocking(true) {
        error!("admin REPL: failed to set listener nonblocking: {err}");
        return;
    }

    if let Err(err) = std::fs::set_permissions(
        &config.socket_path,
        std::fs::Permissions::from_mode(config.mode),
    ) {
        warn!(
            "admin REPL: failed to chmod {} to {:o}: {err}",
            config.socket_path.display(),
            config.mode
        );
    }

    info!(
        "admin REPL listening on UNIX socket {} (mode {:o})",
        config.socket_path.display(),
        config.mode
    );
    state.listener = Some(listener);
}

fn accept_admin_connections(mut state: ResMut<AdminListenerState>) {
    let Some(listener) = state.listener.as_ref().and_then(|l| l.try_clone().ok()) else {
        return;
    };

    loop {
        match listener.accept() {
            Ok((stream, _addr)) => {
                if let Err(err) = stream.set_nonblocking(true) {
                    warn!("admin REPL: set_nonblocking on accepted stream failed: {err}");
                    continue;
                }

                let id = state.next_session_id;
                state.next_session_id += 1;
                let mut session = AdminSession {
                    stream,
                    read_buffer: Vec::new(),
                    pending_input: String::new(),
                    caller: None,
                };
                let banner_payload = format!("{BANNER}\n{PROMPT_NEW}");
                if let Err(err) = session.stream.write_all(banner_payload.as_bytes()) {
                    warn!("admin REPL: banner write failed for session {id}: {err}");
                    continue;
                }
                info!("admin REPL: session {id} connected");
                state.sessions.insert(id, session);
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => break,
            Err(err) => {
                warn!("admin REPL: accept failed: {err}");
                break;
            }
        }
    }
}

fn poll_admin_sessions(
    mut state: ResMut<AdminListenerState>,
    mut host: NonSendMut<AdminReplHost>,
    snapshot_params: WorldSnapshotParams,
    mut pending_commands: ResMut<PendingGameCommands>,
) {
    let session_ids: Vec<u64> = state.sessions.keys().copied().collect();
    let mut to_drop: Vec<u64> = Vec::new();

    for id in session_ids {
        let Some(session) = state.sessions.get_mut(&id) else {
            continue;
        };

        let mut disconnected = false;
        let mut lines: Vec<String> = Vec::new();
        loop {
            match read_admin_line(
                &mut session.stream,
                &mut session.read_buffer,
                &mut disconnected,
            ) {
                AdminReadOutcome::Line(line) => lines.push(line),
                AdminReadOutcome::Pending => break,
                AdminReadOutcome::Closed => {
                    disconnected = true;
                    break;
                }
                AdminReadOutcome::Overflow => {
                    warn!("admin REPL: session {id} exceeded {READ_BUFFER_LIMIT} bytes without a newline; dropping");
                    let _ = session.stream.write_all(b"input too long, disconnecting\n");
                    disconnected = true;
                    break;
                }
            }
        }

        for line in lines {
            // A bare blank line is the user's "execute now / cancel block"
            // signal. Always force a final compile attempt with the blank
            // line appended so `compile_or_incomplete` reports the real
            // error or executes the pending block.
            if line.is_empty() && session.pending_input.is_empty() {
                let _ = session.stream.write_all(PROMPT_NEW.as_bytes());
                continue;
            }
            session.pending_input.push_str(&line);
            session.pending_input.push('\n');

            let outcome = host.compile_or_incomplete(&session.pending_input);
            match outcome {
                CompileOutcome::Incomplete => {
                    let _ = session.stream.write_all(PROMPT_CONT.as_bytes());
                }
                CompileOutcome::SyntaxError(msg) => {
                    let payload = format!("{}\n{PROMPT_NEW}", msg.trim_end());
                    let _ = session.stream.write_all(payload.as_bytes());
                    session.pending_input.clear();
                }
                CompileOutcome::Complete(code) => {
                    let snapshot = snapshot_params.build_for_player(session.caller);
                    let result = host.execute_compiled(code, snapshot, session.caller.map(|p| p.0));

                    let mut payload = String::new();
                    for line in &result.stdout {
                        payload.push_str(line);
                        payload.push('\n');
                    }
                    if let Some(err) = &result.error {
                        payload.push_str(err);
                        payload.push('\n');
                    }
                    payload.push_str(PROMPT_NEW);
                    let _ = session.stream.write_all(payload.as_bytes());

                    for cmd in result.queued_commands {
                        match session.caller {
                            Some(id) => pending_commands.push_for_player(id, cmd),
                            None => pending_commands.push(cmd),
                        }
                    }

                    if let Some(new_caller) = result.attach {
                        session.caller = new_caller.map(PlayerId);
                    }

                    session.pending_input.clear();
                }
            }
        }

        if disconnected {
            to_drop.push(id);
        }
    }

    for id in to_drop {
        if state.sessions.remove(&id).is_some() {
            info!("admin REPL: session {id} disconnected");
        }
    }
}

fn unlink_admin_socket_on_exit(
    mut app_exit: MessageReader<AppExit>,
    config: Option<Res<AdminListenerConfig>>,
) {
    if app_exit.read().next().is_none() {
        return;
    }
    let Some(config) = config else { return };
    if config.socket_path.exists() {
        if let Err(err) = std::fs::remove_file(&config.socket_path) {
            warn!(
                "admin REPL: failed to unlink {}: {err}",
                config.socket_path.display()
            );
        }
    }
}

enum AdminReadOutcome {
    Line(String),
    Pending,
    Closed,
    Overflow,
}

fn read_admin_line(
    stream: &mut UnixStream,
    buffer: &mut Vec<u8>,
    disconnected: &mut bool,
) -> AdminReadOutcome {
    if let Some(line) = take_buffered_line(buffer) {
        return AdminReadOutcome::Line(line);
    }

    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => {
                *disconnected = true;
                return AdminReadOutcome::Closed;
            }
            Ok(n) => {
                buffer.extend_from_slice(&chunk[..n]);
                if buffer.len() > READ_BUFFER_LIMIT {
                    return AdminReadOutcome::Overflow;
                }
                if let Some(line) = take_buffered_line(buffer) {
                    return AdminReadOutcome::Line(line);
                }
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => return AdminReadOutcome::Pending,
            Err(err) => {
                warn!("admin REPL: read error: {err}");
                *disconnected = true;
                return AdminReadOutcome::Closed;
            }
        }
    }
}

fn take_buffered_line(buffer: &mut Vec<u8>) -> Option<String> {
    let idx = buffer.iter().position(|b| *b == b'\n')?;
    let line: Vec<u8> = buffer.drain(..=idx).collect();
    let mut payload = &line[..line.len().saturating_sub(1)];
    if payload.last() == Some(&b'\r') {
        payload = &payload[..payload.len() - 1];
    }
    String::from_utf8(payload.to_vec()).ok()
}

fn clear_stale_socket(path: &std::path::Path) -> std::io::Result<()> {
    if !path.exists() {
        // Ensure the parent directory exists so bind() succeeds.
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        return Ok(());
    }
    let meta = std::fs::symlink_metadata(path)?;
    let file_type = meta.file_type();
    let is_socket = {
        use std::os::unix::fs::FileTypeExt;
        file_type.is_socket()
    };
    if !is_socket {
        return Err(std::io::Error::new(
            ErrorKind::AlreadyExists,
            "path exists and is not a UNIX socket",
        ));
    }
    // The path is a socket. If we can connect to it, someone else is
    // listening — refuse to clobber. Otherwise it's stale and we unlink.
    match UnixStream::connect(path) {
        Ok(_) => Err(std::io::Error::new(
            ErrorKind::AddrInUse,
            "another mud2 admin REPL is already listening on this path",
        )),
        Err(_) => std::fs::remove_file(path),
    }
}

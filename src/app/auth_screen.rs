//! Authenticating-state systems. Takes `PendingAuthRequest` populated by the
//! title screen and drives the auth handshake: send `Login`/`Register`, wait
//! for `AuthResult`, transition to `AssetSync` on success or back to the
//! title screen on failure.

use bevy::log::{info, warn};
use bevy::prelude::*;

use crate::app::state::ClientAppState;
use crate::network::protocol::{ClientMessage, ServerMessage};
use crate::network::resources::{TcpClientConfig, TcpClientConnection};

/// Credentials entered on the title screen, awaiting submission on the
/// Authenticating state. Replaced with a new value on each Connect click.
#[derive(Resource, Clone, Default)]
pub struct PendingAuthRequest {
    pub username: String,
    pub password: String,
    pub is_register: bool,
    /// True once we've written the Login/Register message to the stream.
    pub sent: bool,
    /// Populated by the server's AuthResult; read by the title screen on
    /// re-entry to display the error.
    pub error_message: Option<String>,
}

pub struct AuthScreenPlugin;

impl Plugin for AuthScreenPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PendingAuthRequest::default())
            .add_systems(OnEnter(ClientAppState::Authenticating), reset_send_flag)
            .add_systems(
                Update,
                (submit_pending_auth, poll_auth_result)
                    .run_if(in_state(ClientAppState::Authenticating)),
            );
    }
}

fn reset_send_flag(mut pending: ResMut<PendingAuthRequest>) {
    pending.sent = false;
    pending.error_message = None;
}

fn submit_pending_auth(
    mut pending: ResMut<PendingAuthRequest>,
    config: Res<TcpClientConfig>,
    mut connection: ResMut<TcpClientConnection>,
) {
    if pending.sent {
        return;
    }

    // Auth runs before the AssetSync state, so the connect helper isn't
    // wired into any of the polling systems that would otherwise cover it;
    // drive it directly here.
    crate::network::systems::ensure_tcp_client_connected(&config, &mut connection);

    // The stream may still be waking up on the first tick after Connect; wait
    // for it to be available.
    let Some(stream) = connection.stream.as_mut() else {
        return;
    };

    let msg = if pending.is_register {
        ClientMessage::Register {
            username: pending.username.clone(),
            password: pending.password.clone(),
        }
    } else {
        ClientMessage::Login {
            username: pending.username.clone(),
            password: pending.password.clone(),
        }
    };

    let mut disconnected = false;
    let sent_ok = crate::network::systems::write_message(stream, &msg, &mut disconnected);
    if sent_ok {
        info!(
            "auth: sent {} for {} to {}",
            if pending.is_register {
                "Register"
            } else {
                "Login"
            },
            pending.username,
            config.server_addr
        );
        pending.sent = true;
    } else if disconnected {
        warn!("auth: lost connection before sending credentials");
        connection.stream = None;
        connection.read_buffer.clear();
        pending.error_message = Some("connection lost".to_owned());
    }
}

fn poll_auth_result(
    mut pending: ResMut<PendingAuthRequest>,
    config: Res<TcpClientConfig>,
    mut connection: ResMut<TcpClientConnection>,
    mut next_state: ResMut<NextState<ClientAppState>>,
) {
    crate::network::systems::ensure_tcp_client_connected(&config, &mut connection);
    let mut read_buffer = std::mem::take(&mut connection.read_buffer);
    let Some(stream) = connection.stream.as_mut() else {
        connection.read_buffer = read_buffer;
        return;
    };

    let mut disconnected = false;
    let mut result: Option<ServerMessage> = None;
    while let Some(line) =
        crate::network::systems::read_next_line(stream, &mut read_buffer, &mut disconnected)
    {
        match serde_json::from_str::<ServerMessage>(&line) {
            Ok(ServerMessage::AuthResult { ok, reason }) => {
                result = Some(ServerMessage::AuthResult { ok, reason });
                break;
            }
            Ok(_) => {
                // Shouldn't arrive before AuthResult, but tolerate extra
                // messages (server won't send asset manifest pre-auth anyway).
            }
            Err(err) => warn!("auth: failed to parse server message: {err}"),
        }
    }

    if disconnected {
        warn!("auth: lost TCP connection to {}", config.server_addr);
        connection.stream = None;
        connection.read_buffer.clear();
        pending.error_message = Some("connection lost".to_owned());
        next_state.set(ClientAppState::TitleScreen);
        return;
    } else {
        connection.read_buffer = read_buffer;
    }

    match result {
        Some(ServerMessage::AuthResult { ok: true, .. }) => {
            info!("auth: accepted by {}", config.server_addr);
            pending.error_message = None;
            next_state.set(ClientAppState::AssetSync);
        }
        Some(ServerMessage::AuthResult { ok: false, reason }) => {
            let message = reason.unwrap_or_else(|| "auth rejected".to_owned());
            warn!("auth: rejected by {}: {}", config.server_addr, message);
            pending.error_message = Some(message);
            connection.stream = None;
            connection.read_buffer.clear();
            next_state.set(ClientAppState::TitleScreen);
        }
        _ => {}
    }
}

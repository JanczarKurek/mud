//! CLI-driven login autopilot for test scenarios.
//!
//! `--auto-login` / `--auto-character` skip the title-screen flow: the
//! autopilot drives the exact same resources the buttons do (auth handshake,
//! `ListCharacters` / `CreateCharacter` / `SelectCharacter` over the wire), so
//! nothing is bypassed — it is a robot user, not a side channel. Used by
//! `scripts/multiplayer_test.sh` to boot a server plus N logged-in clients.

use bevy::log::{error, info, warn};
use bevy::prelude::*;

use crate::app::auth_screen::PendingAuthRequest;
use crate::app::character_select_screen::CharacterSelectState;
use crate::app::plugin::AppRuntime;
use crate::app::state::ClientAppState;
use crate::network::protocol::ClientMessage;
use crate::network::resources::{TcpClientConfig, TcpClientConnection};
use crate::player::classes::Class;
use crate::player::components::{AttributeSet, PlayerAppearance};

/// Resolved from the `--auto-*` CLI flags (see `app::cli`).
#[derive(Clone, Debug)]
pub struct AutopilotConfig {
    /// Account credentials. `None` in EmbeddedClient mode, where the loopback
    /// pipe is the trust model and Login/Register are skipped.
    pub username: Option<String>,
    pub password: String,
    /// Character to select once at the roster; created if missing.
    pub character: String,
    /// Class used only when the character has to be created.
    pub class: Class,
}

/// Give up on transient (connection-level) auth failures after this many
/// attempts, one per second — generous enough for a server that is still
/// compiling/booting when the client starts.
const MAX_AUTH_ATTEMPTS: u32 = 120;

enum Stage {
    /// Not started yet (first frames on the title screen).
    Start,
    /// Login/Register handed to the auth screen; if we find ourselves back on
    /// the title screen in this stage, the attempt failed.
    AuthPending { registered: bool },
    /// At character select: waiting for the roster, creating the character if
    /// it is missing, then selecting it.
    SelectPending { create_sent: bool },
    /// Terminal: either in game or gave up (manual control from here).
    Done,
}

#[derive(Resource)]
struct AutopilotState {
    config: AutopilotConfig,
    stage: Stage,
    /// Paces retries of transient auth failures (server not up yet).
    retry_timer: Timer,
    attempts: u32,
}

pub struct AutopilotPlugin {
    pub config: AutopilotConfig,
}

impl Plugin for AutopilotPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(AutopilotState {
            config: self.config.clone(),
            stage: Stage::Start,
            retry_timer: Timer::from_seconds(1.0, TimerMode::Repeating),
            attempts: 0,
        })
        .add_systems(
            Update,
            autopilot_title_screen.run_if(in_state(ClientAppState::TitleScreen)),
        )
        .add_systems(
            Update,
            autopilot_character_select.run_if(in_state(ClientAppState::CharacterSelect)),
        );
    }
}

/// Point-buy-legal starting spread per class, used when the autopilot has to
/// create the character. Kept valid by the unit test below.
fn class_default_attributes(class: Class) -> AttributeSet {
    match class {
        Class::Fighter => AttributeSet::new(14, 12, 14, 10, 10, 12),
        Class::Wizard => AttributeSet::new(10, 10, 12, 14, 10, 16),
        Class::Cleric => AttributeSet::new(12, 10, 12, 16, 12, 10),
        Class::Vagabond => AttributeSet::new(10, 16, 12, 10, 12, 12),
    }
}

/// Credential rejections that a retry cannot fix; everything else (connection
/// refused, connection lost, ...) is treated as transient.
fn is_fatal_auth_error(reason: &str) -> bool {
    ["wrong password", "username already taken", "invalid"]
        .iter()
        .any(|needle| reason.contains(needle))
}

fn begin_auth(
    username: &str,
    password: &str,
    is_register: bool,
    config: &mut TcpClientConfig,
    connection: &mut TcpClientConnection,
    pending: &mut PendingAuthRequest,
    next_state: &mut NextState<ClientAppState>,
) {
    // Mirrors the title screen's Connect handler: activate the dial (a failed
    // attempt deactivates it), clear the previous failure, hand credentials to
    // the auth screen.
    config.active = true;
    connection.connect_attempted = false;
    connection.error_message = None;
    pending.username = username.to_owned();
    pending.password = password.to_owned();
    pending.is_register = is_register;
    pending.sent = false;
    pending.error_message = None;
    next_state.set(ClientAppState::Authenticating);
}

fn autopilot_title_screen(
    mut autopilot: ResMut<AutopilotState>,
    runtime: Res<AppRuntime>,
    time: Res<Time>,
    mut tcp_config: Option<ResMut<TcpClientConfig>>,
    mut connection: Option<ResMut<TcpClientConnection>>,
    mut pending_auth: Option<ResMut<PendingAuthRequest>>,
    #[cfg(feature = "server-sim")] mut server_state: Option<
        ResMut<crate::network::resources::TcpServerState>,
    >,
    mut next_state: ResMut<NextState<ClientAppState>>,
) {
    // EmbeddedClient: no credentials — wire the loopback pipe exactly like the
    // Play button and head straight to character select.
    if matches!(*runtime, AppRuntime::EmbeddedClient) {
        if matches!(autopilot.stage, Stage::Start) {
            #[cfg(feature = "server-sim")]
            if let (Some(server_state), Some(connection)) =
                (server_state.as_mut(), connection.as_mut())
            {
                info!(
                    "autopilot: embedded loopback connect, heading to character '{}'",
                    autopilot.config.character
                );
                crate::network::loopback::connect_loopback(server_state, connection);
                next_state.set(ClientAppState::CharacterSelect);
                autopilot.stage = Stage::SelectPending { create_sent: false };
            }
        }
        return;
    }

    let (Some(config), Some(connection), Some(pending)) = (
        tcp_config.as_deref_mut(),
        connection.as_deref_mut(),
        pending_auth.as_deref_mut(),
    ) else {
        return;
    };
    let Some(username) = autopilot.config.username.clone() else {
        return;
    };
    let password = autopilot.config.password.clone();

    match autopilot.stage {
        Stage::Start => {
            info!(
                "autopilot: logging in as '{username}' at {}",
                config.server_addr
            );
            begin_auth(
                &username,
                &password,
                false,
                config,
                connection,
                pending,
                &mut next_state,
            );
            autopilot.stage = Stage::AuthPending { registered: false };
        }
        Stage::AuthPending { registered } => {
            // Back on the title screen while an auth was pending — it failed;
            // the reason is what the title screen would display.
            let reason = pending
                .error_message
                .clone()
                .unwrap_or_else(|| "unknown error".to_owned());

            if reason.contains("unknown user") && !registered {
                info!("autopilot: account '{username}' does not exist; registering");
                begin_auth(
                    &username,
                    &password,
                    true,
                    config,
                    connection,
                    pending,
                    &mut next_state,
                );
                autopilot.stage = Stage::AuthPending { registered: true };
            } else if is_fatal_auth_error(&reason) {
                error!("autopilot: giving up, auth rejected: {reason}");
                autopilot.stage = Stage::Done;
            } else {
                // Transient (server not up yet, connection lost): retry at
                // 1 Hz until the attempt budget runs out.
                if !autopilot.retry_timer.tick(time.delta()).just_finished() {
                    return;
                }
                autopilot.attempts += 1;
                if autopilot.attempts >= MAX_AUTH_ATTEMPTS {
                    error!(
                        "autopilot: giving up after {} attempts: {reason}",
                        autopilot.attempts
                    );
                    autopilot.stage = Stage::Done;
                    return;
                }
                info!(
                    "autopilot: retrying login (attempt {}): {reason}",
                    autopilot.attempts + 1
                );
                begin_auth(
                    &username,
                    &password,
                    registered,
                    config,
                    connection,
                    pending,
                    &mut next_state,
                );
            }
        }
        // Bounced back to the title screen after character select (e.g. a
        // manual logout); the user has taken over.
        Stage::SelectPending { .. } | Stage::Done => {}
    }
}

fn autopilot_character_select(
    mut autopilot: ResMut<AutopilotState>,
    mut select_state: ResMut<CharacterSelectState>,
    config: Option<Res<TcpClientConfig>>,
    mut connection: Option<ResMut<TcpClientConnection>>,
    mut next_state: ResMut<NextState<ClientAppState>>,
) {
    // Arriving here from the auth flow means auth succeeded.
    if matches!(autopilot.stage, Stage::AuthPending { .. }) {
        autopilot.stage = Stage::SelectPending { create_sent: false };
    }
    let Stage::SelectPending { create_sent } = autopilot.stage else {
        return;
    };

    // `request_character_list` (OnEnter) already asked for the roster; wait
    // for the reply so an empty roster is distinguishable from a pending one.
    if !select_state.roster_loaded {
        return;
    }

    let wanted = autopilot.config.character.clone();
    let found = select_state
        .characters
        .iter()
        .find(|c| c.name == wanted)
        .map(|c| c.character_id);
    if let Some(character_id) = found {
        info!("autopilot: selecting character '{wanted}' (id {character_id})");
        select_state.selected_character_id = Some(character_id);
        send_message(
            config.as_deref(),
            connection.as_deref_mut(),
            ClientMessage::SelectCharacter {
                character_id,
                start_map: None,
            },
        );
        next_state.set(ClientAppState::AssetSync);
        autopilot.stage = Stage::Done;
        return;
    }

    if !create_sent {
        let class = autopilot.config.class;
        info!(
            "autopilot: creating character '{wanted}' ({})",
            class.label()
        );
        send_message(
            config.as_deref(),
            connection.as_deref_mut(),
            ClientMessage::CreateCharacter {
                name: wanted,
                class,
                attributes: class_default_attributes(class),
                appearance: PlayerAppearance::default(),
            },
        );
        // On success the server pushes a fresh CharacterList and the branch
        // above selects the character on a later frame.
        autopilot.stage = Stage::SelectPending { create_sent: true };
    } else if let Some(reason) = select_state.error_message.clone() {
        error!("autopilot: character create failed: {reason}");
        autopilot.stage = Stage::Done;
    }
}

fn send_message(
    config: Option<&TcpClientConfig>,
    connection: Option<&mut TcpClientConnection>,
    msg: ClientMessage,
) {
    let (Some(config), Some(connection)) = (config, connection) else {
        return;
    };
    crate::network::systems::ensure_tcp_client_connected(config, connection);
    let Some(stream) = connection.stream.as_mut() else {
        warn!("autopilot: no connection to send {msg:?}");
        return;
    };
    let mut disconnected = false;
    crate::network::systems::write_message(stream, &msg, &mut disconnected);
    if disconnected {
        connection.stream = None;
        connection.read_buffer.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::components::validate_point_buy;

    #[test]
    fn class_default_attributes_pass_point_buy() {
        for class in Class::ALL {
            validate_point_buy(&class_default_attributes(class)).unwrap_or_else(|err| {
                panic!("{} default attributes invalid: {err}", class.label())
            });
        }
    }
}

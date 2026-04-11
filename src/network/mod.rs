pub mod protocol;
pub mod resources;
pub mod systems;

use bevy::prelude::*;

use crate::game::systems::{apply_game_events_to_client_state, process_game_commands};
use crate::network::resources::{
    TcpClientConfig, TcpClientConnection, TcpServerConfig, TcpServerState,
};
use crate::network::systems::{
    accept_tcp_client_connections, flush_client_commands_to_server, flush_server_messages,
    poll_tcp_client_messages, poll_tcp_server_messages, start_tcp_server,
};

pub struct TcpClientPlugin {
    pub server_addr: String,
}

pub struct TcpServerPlugin {
    pub bind_addr: String,
}

impl Plugin for TcpClientPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(TcpClientConfig {
            server_addr: self.server_addr.clone(),
        })
        .insert_resource(TcpClientConnection::default())
        .add_systems(Update, flush_client_commands_to_server)
        .add_systems(
            Update,
            poll_tcp_client_messages.before(apply_game_events_to_client_state),
        );
    }
}

impl Plugin for TcpServerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(TcpServerConfig {
            bind_addr: self.bind_addr.clone(),
        })
        .insert_resource(TcpServerState::default())
        .add_systems(Startup, start_tcp_server)
        .add_systems(Update, accept_tcp_client_connections)
        .add_systems(
            Update,
            poll_tcp_server_messages.before(process_game_commands),
        )
        .add_systems(
            Update,
            flush_server_messages.after(apply_game_events_to_client_state),
        );
    }
}

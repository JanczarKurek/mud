pub mod asset_sync;
pub mod protocol;
pub mod resources;
pub mod systems;

use bevy::prelude::*;

use crate::app::state::ClientAppState;
use crate::game::projection::apply_game_events_to_client_state;
use crate::game::systems::process_game_commands;
use crate::network::resources::{
    AssetSyncState, TcpClientConfig, TcpClientConnection, TcpServerConfig, TcpServerState,
};
use crate::network::systems::{
    accept_tcp_client_connections, build_and_store_manifest, flush_client_commands_to_server,
    flush_server_messages, poll_tcp_asset_sync_messages, poll_tcp_client_messages,
    poll_tcp_server_messages, send_asset_manifest_to_new_peers, start_tcp_server,
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
            active: false,
        })
        .insert_resource(TcpClientConnection::default())
        .insert_resource(AssetSyncState::default())
        .add_systems(
            Update,
            poll_tcp_asset_sync_messages.run_if(in_state(ClientAppState::AssetSync)),
        )
        .add_systems(
            Update,
            flush_client_commands_to_server.run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            Update,
            poll_tcp_client_messages
                .before(apply_game_events_to_client_state)
                .run_if(in_state(ClientAppState::InGame)),
        );
    }
}

impl Plugin for TcpServerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(TcpServerConfig {
            bind_addr: self.bind_addr.clone(),
        })
        .insert_resource(TcpServerState::default())
        .add_systems(Startup, (start_tcp_server, build_and_store_manifest))
        .add_systems(Update, accept_tcp_client_connections)
        .add_systems(Update, send_asset_manifest_to_new_peers)
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

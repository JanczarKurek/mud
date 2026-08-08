#[cfg(all(unix, feature = "server-sim"))]
pub mod admin;
pub mod asset_sync;
pub mod loopback;
pub mod protocol;
pub mod resources;
pub mod sets;
pub mod systems;
pub mod transport;

#[cfg(all(unix, feature = "server-sim"))]
pub use crate::network::admin::{AdminListenArgs, AdminReplPlugin};

#[cfg(feature = "server-sim")]
use std::sync::Arc;

use bevy::prelude::*;
#[cfg(feature = "server-sim")]
use rustls::ServerConfig;

use crate::app::state::ClientAppState;
use crate::game::projection::apply_game_events_to_client_state;
#[cfg(feature = "server-sim")]
use crate::game::systems::process_game_commands;
use crate::network::resources::{
    AssetSyncState, TcpClientConfig, TcpClientConnection, TcpClientTlsConfig,
};
#[cfg(feature = "server-sim")]
use crate::network::resources::{LatencyReportTimer, PingTimer, TcpServerConfig, TcpServerState};
#[cfg(feature = "server-sim")]
use crate::network::systems::{
    accept_tcp_client_connections, build_and_store_manifest, flush_server_messages,
    poll_tcp_server_messages, report_peer_latency, send_asset_manifest_to_new_peers,
    send_periodic_pings, start_tcp_server,
};
use crate::network::systems::{
    flush_client_commands_to_server, poll_tcp_asset_sync_messages, poll_tcp_client_messages,
};

pub struct TcpClientPlugin {
    pub server_addr: String,
    /// When `Some`, the client wraps its outgoing connection in TLS. The
    /// `server_name` inside is passed as the SNI hostname.
    pub tls: Option<TcpClientTlsConfig>,
}

#[cfg(feature = "server-sim")]
pub struct TcpServerPlugin {
    /// `Some(addr)` binds a real TCP listener; `None` runs the server systems
    /// with no listener — peers arrive only via `loopback::connect_loopback`
    /// (the EmbeddedClient configuration).
    pub bind_addr: Option<String>,
    /// When `Some`, accepted connections are wrapped in TLS.
    pub tls_config: Option<Arc<ServerConfig>>,
}

impl Plugin for TcpClientPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(TcpClientConfig {
            server_addr: self.server_addr.clone(),
            active: false,
            tls: self.tls.clone(),
        })
        .insert_resource(TcpClientConnection::default())
        .insert_resource(AssetSyncState::default())
        // The ungated pipeline chain (see `network::sets`). The server-side
        // sets are configured here — not in the gated server plugins — so the
        // edges exist in every mode; empty sets are no-ops.
        .configure_sets(
            Update,
            (
                sets::NetClientSend,
                sets::NetServerReceive,
                sets::NetServerSend,
                sets::NetClientReceive,
            )
                .chain(),
        )
        .add_systems(
            Update,
            poll_tcp_asset_sync_messages.run_if(in_state(ClientAppState::AssetSync)),
        )
        .add_systems(
            Update,
            flush_client_commands_to_server
                .in_set(sets::NetClientSend)
                .run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            Update,
            poll_tcp_client_messages
                .in_set(sets::NetClientReceive)
                .before(apply_game_events_to_client_state)
                .run_if(in_state(ClientAppState::InGame)),
        );
    }
}

#[cfg(feature = "server-sim")]
impl Plugin for TcpServerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(TcpServerConfig {
            bind_addr: self.bind_addr.clone(),
            tls_config: self.tls_config.clone(),
        })
        .insert_resource(TcpServerState::default())
        .insert_resource(PingTimer::default())
        .insert_resource(LatencyReportTimer::default())
        .add_systems(Startup, (start_tcp_server, build_and_store_manifest))
        .add_systems(Update, accept_tcp_client_connections)
        .add_systems(Update, send_asset_manifest_to_new_peers)
        .add_systems(
            Update,
            // Must run before the whole `CommandIntercept` set, not just
            // `process_game_commands`: intercept systems (dialog, trade, chat,
            // rotate, …) drain their command variants ahead of the main
            // dispatcher, and a command ingested between an intercept system
            // and `process_game_commands` would reach the dispatcher's
            // catch-all and be dropped ("saw <variant> — check system
            // ordering"). The set edge also orders us before
            // `process_game_commands` transitively; the direct `.before` stays
            // as a belt-and-suspenders for apps without the set configured.
            poll_tcp_server_messages
                .in_set(sets::NetServerReceive)
                .before(crate::game::CommandIntercept)
                .before(process_game_commands),
        )
        .add_systems(
            Update,
            // Ordering: after the frame's simulation (anchored by
            // `GameServerPlugin`'s `NetServerSend` config) and before the
            // client-side receive+fold (the `network::sets` chain). The old
            // `.after(apply_game_events_to_client_state)` edge is gone — in a
            // unified App the client fold must run *after* the server flush so
            // events cross the loopback within the same frame.
            flush_server_messages.in_set(sets::NetServerSend),
        )
        .add_systems(Update, (send_periodic_pings, report_peer_latency));
    }
}

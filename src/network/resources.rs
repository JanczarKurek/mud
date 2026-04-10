use std::net::{TcpListener, TcpStream};

use bevy::prelude::*;

use crate::game::resources::ClientGameState;

#[derive(Resource)]
pub struct TcpClientConfig {
    pub server_addr: String,
}

#[derive(Resource, Default)]
pub struct TcpClientConnection {
    pub stream: Option<TcpStream>,
    pub read_buffer: Vec<u8>,
}

#[derive(Resource)]
pub struct TcpServerConfig {
    pub bind_addr: String,
}

pub struct TcpServerPeer {
    pub stream: TcpStream,
    pub read_buffer: Vec<u8>,
    pub last_snapshot: Option<ClientGameState>,
}

#[derive(Resource, Default)]
pub struct TcpServerState {
    pub listener: Option<TcpListener>,
    pub client: Option<TcpServerPeer>,
}

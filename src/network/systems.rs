use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};

use bevy::log::{error, info, warn};
use bevy::prelude::*;

use crate::game::resources::{ClientGameState, PendingGameCommands, PendingGameUiEvents};
use crate::network::protocol::{ClientMessage, ServerMessage};
use crate::network::resources::{
    TcpClientConfig, TcpClientConnection, TcpServerConfig, TcpServerPeer, TcpServerState,
};

pub fn start_tcp_server(config: Res<TcpServerConfig>, mut server_state: ResMut<TcpServerState>) {
    if server_state.listener.is_some() {
        return;
    }

    let Ok(listener) = TcpListener::bind(&config.bind_addr) else {
        error!("failed to bind TCP server on {}", config.bind_addr);
        return;
    };
    if let Err(error) = listener.set_nonblocking(true) {
        error!("failed to set TCP listener nonblocking: {error}");
        return;
    }

    info!("TCP server listening on {}", config.bind_addr);
    server_state.listener = Some(listener);
}

pub fn accept_tcp_client_connections(mut server_state: ResMut<TcpServerState>) {
    let Some(listener) = server_state
        .listener
        .as_ref()
        .and_then(|listener| listener.try_clone().ok())
    else {
        return;
    };

    loop {
        match listener.accept() {
            Ok((stream, address)) => {
                if let Err(error) = stream.set_nonblocking(true) {
                    warn!("failed to set accepted stream nonblocking: {error}");
                    continue;
                }

                info!("TCP client connected from {address}");
                server_state.client = Some(TcpServerPeer {
                    stream,
                    read_buffer: Vec::new(),
                    last_snapshot: None,
                });
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => break,
            Err(error) => {
                warn!("TCP accept failed: {error}");
                break;
            }
        }
    }
}

pub fn poll_tcp_server_messages(
    mut server_state: ResMut<TcpServerState>,
    mut pending_commands: ResMut<PendingGameCommands>,
) {
    let Some(client) = &mut server_state.client else {
        return;
    };

    let mut disconnected = false;
    while let Some(line) = read_next_line(&mut client.stream, &mut client.read_buffer, &mut disconnected)
    {
        match serde_json::from_str::<ClientMessage>(&line) {
            Ok(ClientMessage::Command(command)) => pending_commands.push(command),
            Err(error) => warn!("failed to parse client message: {error}"),
        }
    }

    if disconnected {
        info!("TCP client disconnected");
        server_state.client = None;
    }
}

pub fn flush_server_messages(
    client_state: Res<ClientGameState>,
    mut pending_ui_events: ResMut<PendingGameUiEvents>,
    mut server_state: ResMut<TcpServerState>,
) {
    let Some(client) = &mut server_state.client else {
        pending_ui_events.events.clear();
        return;
    };

    let mut disconnected = false;
    if client.last_snapshot.as_ref() != Some(&*client_state) {
        if !write_message(&mut client.stream, &ServerMessage::Snapshot(client_state.clone()), &mut disconnected)
        {
            warn!("failed to send snapshot to TCP client");
        } else {
            client.last_snapshot = Some(client_state.clone());
        }
    }

    if !pending_ui_events.events.is_empty() {
        let events = std::mem::take(&mut pending_ui_events.events);
        if !write_message(&mut client.stream, &ServerMessage::UiEvents(events), &mut disconnected) {
            warn!("failed to send UI events to TCP client");
        }
    }

    if disconnected {
        info!("TCP client disconnected");
        server_state.client = None;
    }
}

pub fn flush_client_commands_to_server(
    config: Res<TcpClientConfig>,
    mut connection: ResMut<TcpClientConnection>,
    mut pending_commands: ResMut<PendingGameCommands>,
) {
    ensure_tcp_client_connected(&config, &mut connection);

    let Some(stream) = &mut connection.stream else {
        pending_commands.commands.clear();
        return;
    };

    let commands = std::mem::take(&mut pending_commands.commands);
    let mut disconnected = false;
    for command in commands {
        if !write_message(stream, &ClientMessage::Command(command), &mut disconnected) {
            break;
        }
    }

    if disconnected {
        connection.stream = None;
        connection.read_buffer.clear();
    }
}

pub fn poll_tcp_client_messages(
    config: Res<TcpClientConfig>,
    mut connection: ResMut<TcpClientConnection>,
    mut client_state: ResMut<ClientGameState>,
    mut pending_ui_events: ResMut<PendingGameUiEvents>,
) {
    ensure_tcp_client_connected(&config, &mut connection);

    let mut read_buffer = std::mem::take(&mut connection.read_buffer);
    let Some(stream) = &mut connection.stream else {
        connection.read_buffer = read_buffer;
        return;
    };

    let mut disconnected = false;
    while let Some(line) = read_next_line(stream, &mut read_buffer, &mut disconnected) {
        match serde_json::from_str::<ServerMessage>(&line) {
            Ok(ServerMessage::Snapshot(snapshot)) => {
                *client_state = snapshot;
            }
            Ok(ServerMessage::UiEvents(events)) => {
                pending_ui_events.events.extend(events);
            }
            Err(error) => warn!("failed to parse server message: {error}"),
        }
    }

    if disconnected {
        warn!("lost TCP connection to {}", config.server_addr);
        connection.stream = None;
        connection.read_buffer.clear();
    } else {
        connection.read_buffer = read_buffer;
    }
}

fn ensure_tcp_client_connected(config: &TcpClientConfig, connection: &mut TcpClientConnection) {
    if connection.stream.is_some() {
        return;
    }

    let Ok(stream) = TcpStream::connect(&config.server_addr) else {
        return;
    };
    if let Err(error) = stream.set_nonblocking(true) {
        warn!("failed to set TCP client stream nonblocking: {error}");
        return;
    }

    info!("connected to TCP server at {}", config.server_addr);
    connection.stream = Some(stream);
}

fn read_next_line(
    stream: &mut TcpStream,
    buffer: &mut Vec<u8>,
    disconnected: &mut bool,
) -> Option<String> {
    let mut chunk = [0; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => {
                *disconnected = true;
                break;
            }
            Ok(count) => buffer.extend_from_slice(&chunk[..count]),
            Err(error) if error.kind() == ErrorKind::WouldBlock => break,
            Err(error) => {
                warn!("TCP read failed: {error}");
                *disconnected = true;
                break;
            }
        }
    }

    let newline_index = buffer.iter().position(|byte| *byte == b'\n')?;
    let line = buffer.drain(..=newline_index).collect::<Vec<_>>();
    let payload = &line[..line.len().saturating_sub(1)];
    String::from_utf8(payload.to_vec()).ok()
}

fn write_message(
    stream: &mut TcpStream,
    message: &impl serde::Serialize,
    disconnected: &mut bool,
) -> bool {
    let Ok(mut bytes) = serde_json::to_vec(message) else {
        return false;
    };
    bytes.push(b'\n');

    match stream.write_all(&bytes) {
        Ok(()) => true,
        Err(error) if error.kind() == ErrorKind::WouldBlock => false,
        Err(error) => {
            warn!("TCP write failed: {error}");
            *disconnected = true;
            false
        }
    }
}

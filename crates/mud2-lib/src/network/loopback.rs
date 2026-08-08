//! In-process byte-pipe transport. A [`loopback_pair`] returns two connected
//! endpoints, each implementing `Read` + `Write` with the same sync
//! nonblocking semantics as a `TcpStream` (`WouldBlock` when empty, `Ok(0)`
//! EOF after the peer closes). Plugged into `ServerTransport::Loopback` /
//! `ClientTransport::Loopback`, it lets the EmbeddedClient run the *real*
//! client/server message systems — newline-framed serde_json included —
//! without a socket, so offline play exercises the exact wire code path.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// One direction of the pipe: a byte queue plus a closed flag set when the
/// writing side drops.
struct LoopbackBuffer {
    data: Mutex<VecDeque<u8>>,
    closed: AtomicBool,
}

impl LoopbackBuffer {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            data: Mutex::new(VecDeque::new()),
            closed: AtomicBool::new(false),
        })
    }
}

/// One side of an in-process duplex byte pipe. Reads drain `rx`; writes append
/// to `tx` (the peer's `rx`). Dropping an endpoint closes its `tx`, so the
/// peer observes EOF exactly like a closed TCP connection.
pub struct LoopbackEndpoint {
    rx: Arc<LoopbackBuffer>,
    tx: Arc<LoopbackBuffer>,
}

/// Create a connected pair of endpoints.
pub fn loopback_pair() -> (LoopbackEndpoint, LoopbackEndpoint) {
    let a_to_b = LoopbackBuffer::new();
    let b_to_a = LoopbackBuffer::new();
    (
        LoopbackEndpoint {
            rx: Arc::clone(&b_to_a),
            tx: Arc::clone(&a_to_b),
        },
        LoopbackEndpoint {
            rx: a_to_b,
            tx: b_to_a,
        },
    )
}

impl Read for LoopbackEndpoint {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut data = self.rx.data.lock().expect("loopback buffer poisoned");
        if data.is_empty() {
            // Match nonblocking-socket semantics: EOF only once the peer has
            // closed *and* everything it wrote has been drained.
            if self.rx.closed.load(Ordering::Acquire) {
                return Ok(0);
            }
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "loopback pipe empty",
            ));
        }
        let n = buf.len().min(data.len());
        for slot in buf.iter_mut().take(n) {
            *slot = data.pop_front().expect("len checked above");
        }
        Ok(n)
    }
}

impl Write for LoopbackEndpoint {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.tx.closed.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "loopback peer closed",
            ));
        }
        let mut data = self.tx.data.lock().expect("loopback buffer poisoned");
        data.extend(buf.iter().copied());
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for LoopbackEndpoint {
    fn drop(&mut self) {
        self.tx.closed.store(true, Ordering::Release);
        // Also mark our rx closed so a peer that cloned nothing can't spin on
        // a buffer no one will ever drain again.
        self.rx.closed.store(true, Ordering::Release);
    }
}

/// Wire an in-process client "connection" into a running server: creates a
/// loopback pair, registers the server half as a peer, and installs the
/// client half as the active `TcpClientConnection` stream.
///
/// The peer is born `AwaitingCharacter { account_id: LOCAL_ACCOUNT_ID }` —
/// Login/Register are skipped by construction. The pipe existing in-process
/// *is* the trust model, and the reserved local account row has no password
/// hash, so credential auth could never succeed for it anyway. Everything
/// from `ListCharacters` on is real wire traffic.
#[cfg(feature = "server-sim")]
pub fn connect_loopback(
    server_state: &mut crate::network::resources::TcpServerState,
    connection: &mut crate::network::resources::TcpClientConnection,
) {
    use crate::network::resources::{
        ConnectionId, PeerAuthState, PeerLatencyState, PeerThroughputState, TcpServerPeer,
    };
    use crate::network::transport::{ClientTransport, ServerTransport};

    let (server_end, client_end) = loopback_pair();

    let connection_id = ConnectionId(server_state.next_connection_id);
    server_state.next_connection_id += 1;
    bevy::log::info!(
        "loopback client connected (peer {}, awaiting character)",
        connection_id.0
    );
    server_state.peers.insert(
        connection_id,
        TcpServerPeer {
            connection_id,
            auth_state: PeerAuthState::AwaitingCharacter {
                account_id: crate::accounts::db::LOCAL_ACCOUNT_ID,
            },
            remote_addr: None,
            player_id: None,
            player_entity: None,
            // The pipe is in-process: the local account is always an admin.
            is_admin: true,
            stream: ServerTransport::Loopback(server_end),
            read_buffer: Vec::new(),
            last_projection: None,
            floor_diff_cache: Default::default(),
            sync_complete: false,
            manifest_sent: false,
            latency: PeerLatencyState::default(),
            throughput: PeerThroughputState {
                last_report_at: Some(std::time::Instant::now()),
                ..Default::default()
            },
        },
    );

    connection.stream = Some(ClientTransport::Loopback(client_end));
    connection.read_buffer.clear();
    // The dialer must never redial over a live loopback pipe.
    connection.connect_attempted = true;
    connection.error_message = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_wouldblock() {
        let (mut a, mut b) = loopback_pair();
        let mut buf = [0u8; 16];
        assert_eq!(
            b.read(&mut buf).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        a.write_all(b"hello\n").unwrap();
        let n = b.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"hello\n");
    }

    #[test]
    fn drop_signals_eof_after_drain() {
        let (mut a, mut b) = loopback_pair();
        a.write_all(b"bye").unwrap();
        drop(a);
        let mut buf = [0u8; 16];
        // Buffered bytes are still readable...
        let n = b.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"bye");
        // ...then EOF.
        assert_eq!(b.read(&mut buf).unwrap(), 0);
        // And writes to a closed peer fail.
        assert_eq!(b.write(b"x").unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    }
}

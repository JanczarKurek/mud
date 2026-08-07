//! Abstraction over a peer's bytestream so the same `read_next_line` /
//! `write_message` helpers drive plain TCP or TLS transparently. Stays sync +
//! nonblocking to match the existing Bevy-poll model (no tokio).

use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::{Arc, Once};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct, RootCertStore, ServerConfig,
    ServerConnection, SignatureScheme, StreamOwned,
};

/// Peer transport on the server side. One per accepted connection.
pub enum ServerTransport {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ServerConnection, TcpStream>>),
}

impl ServerTransport {
    /// True while the underlying TLS connection is still handshaking. Always
    /// `false` for plain TCP. Used by the read/write wrappers to disambiguate
    /// a mid-handshake `Ok(0)` from a real EOF.
    pub fn is_handshaking(&self) -> bool {
        match self {
            ServerTransport::Plain(_) => false,
            ServerTransport::Tls(stream) => stream.conn.is_handshaking(),
        }
    }
}

impl Read for ServerTransport {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            ServerTransport::Plain(stream) => stream.read(buf),
            ServerTransport::Tls(stream) => {
                let result = stream.read(buf);
                // rustls can return Ok(0) in two distinct situations: real EOF
                // (peer closed TCP cleanly) *or* handshake just completed and
                // no app data has been received yet. The caller's read loop
                // treats Ok(0) as EOF, so when we're still handshaking we
                // translate it to WouldBlock — "try again next tick" — which
                // matches how plain TCP nonblocking sockets signal "no data
                // available yet."
                if matches!(result, Ok(0)) && stream.conn.is_handshaking() {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "TLS handshake in progress",
                    ));
                }
                result
            }
        }
    }
}

impl Write for ServerTransport {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            ServerTransport::Plain(stream) => stream.write(buf),
            ServerTransport::Tls(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            ServerTransport::Plain(stream) => stream.flush(),
            ServerTransport::Tls(stream) => stream.flush(),
        }
    }
}

/// Peer transport on the client side. One per live outgoing connection.
pub enum ClientTransport {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl ClientTransport {
    pub fn is_handshaking(&self) -> bool {
        match self {
            ClientTransport::Plain(_) => false,
            ClientTransport::Tls(stream) => stream.conn.is_handshaking(),
        }
    }
}

impl Read for ClientTransport {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            ClientTransport::Plain(stream) => stream.read(buf),
            ClientTransport::Tls(stream) => {
                let result = stream.read(buf);
                if matches!(result, Ok(0)) && stream.conn.is_handshaking() {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "TLS handshake in progress",
                    ));
                }
                result
            }
        }
    }
}

impl Write for ClientTransport {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            ClientTransport::Plain(stream) => stream.write(buf),
            ClientTransport::Tls(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            ClientTransport::Plain(stream) => stream.flush(),
            ClientTransport::Tls(stream) => stream.flush(),
        }
    }
}

/// Installs the `ring`-backed rustls crypto provider as the process-wide
/// default if it hasn't been installed yet. Called automatically by the TLS
/// config loaders; safe to call repeatedly.
pub fn ensure_crypto_provider_installed() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // Ignore the result — if something else installed a provider first,
        // that's fine too.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Load a PEM cert chain and matching private key off disk and build an
/// `Arc<ServerConfig>` suitable for `rustls::ServerConnection::new`.
pub fn load_server_tls_config(cert_pem: &Path, key_pem: &Path) -> io::Result<Arc<ServerConfig>> {
    ensure_crypto_provider_installed();

    let certs = {
        let mut reader = BufReader::new(File::open(cert_pem)?);
        rustls_pemfile::certs(&mut reader).collect::<Result<Vec<CertificateDer<'static>>, _>>()?
    };
    if certs.is_empty() {
        return Err(io::Error::other(format!(
            "no certificates found in {}",
            cert_pem.display()
        )));
    }

    let key = {
        let mut reader = BufReader::new(File::open(key_pem)?);
        rustls_pemfile::private_key(&mut reader)?.ok_or_else(|| {
            io::Error::other(format!("no private key found in {}", key_pem.display()))
        })?
    };

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(io::Error::other)?;
    Ok(Arc::new(config))
}

/// Build a client TLS config. When `insecure = true` the client skips all
/// certificate verification and logs a warning on connect — use for localhost
/// development only. Otherwise `webpki-roots` trust anchors are used so the
/// client can talk to servers with publicly-signed certs.
pub fn build_client_tls_config(insecure: bool) -> io::Result<Arc<ClientConfig>> {
    ensure_crypto_provider_installed();

    let config = if insecure {
        ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(InsecureCertVerifier::new()))
            .with_no_client_auth()
    } else {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth()
    };
    Ok(Arc::new(config))
}

/// Certificate verifier that accepts anything. NEVER use against the public
/// internet. Only compiled in for the client so the server cannot
/// accidentally skip verification.
#[derive(Debug)]
struct InsecureCertVerifier {
    supported: rustls::crypto::WebPkiSupportedAlgorithms,
}

impl InsecureCertVerifier {
    fn new() -> Self {
        Self {
            supported: rustls::crypto::ring::default_provider().signature_verification_algorithms,
        }
    }
}

impl ServerCertVerifier for InsecureCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.supported)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.supported)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.supported.supported_schemes()
    }
}

/// Generate a self-signed cert + key for localhost and write both to disk.
/// Gated behind the `dev-self-signed` feature so production builds don't ship
/// a cert generator. Intended for local development only.
#[cfg(feature = "dev-self-signed")]
pub fn generate_self_signed_to_disk(cert_path: &Path, key_path: &Path) -> io::Result<()> {
    let subject_alt_names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    let certified =
        rcgen::generate_simple_self_signed(subject_alt_names).map_err(io::Error::other)?;
    if let Some(parent) = cert_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    if let Some(parent) = key_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(cert_path, certified.cert.pem())?;
    std::fs::write(key_path, certified.key_pair.serialize_pem())?;
    Ok(())
}

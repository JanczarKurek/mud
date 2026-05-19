//! Command-line surface for the `mud2` and `server` binaries.
//!
//! Two top-level parsers (`Mud2Cli`, `ServerCli`) share three flattened
//! [`clap::Args`] structs so the common flags (data paths, TLS, admin socket)
//! stay in lock-step. After [`clap::Parser::parse`], the binary calls
//! [`mud2_into_plugin`] or [`server_into_plugin`] to fold the CLI struct
//! plus per-binary derivations (`tls://` scheme stripping, the
//! `--server`/`--connect` runtime resolution, the admin-socket warning when
//! the wrong runtime is selected) into a [`GameAppPlugin`]. Keeping that
//! logic in one place — instead of in `main()` — means both binaries are
//! near-empty and the special-case rules have one home.
//!
//! See `/Users/jhorecki/.claude/plans/purrfect-painting-parrot.md` for the
//! design rationale; see `clean_cache.rs` for the `Paths` / `CleanCache`
//! subcommands embedded into both top-level parsers.

pub mod octal;

use std::path::PathBuf;

use clap::{Args, Parser};

use crate::app::clean_cache::Command;
use crate::app::paths::default_admin_socket_path;
use crate::app::plugin::{AppRuntime, ClientTlsArgs, GameAppPlugin, ServerTlsArgs};

/// Sentinel written by clap into `admin_socket_raw` when `--admin-socket`
/// appears with no value. Picked to be impossible as a real path component
/// (starts and ends with `__`) and never compared to anything but itself.
const ADMIN_SOCKET_DEFAULT_SENTINEL: &str = "__use_default__";

// ---------------------------------------------------------------------------
// Shared `Args` structs flattened into both binaries.
// ---------------------------------------------------------------------------

/// Filesystem overrides shared by both binaries. Env-var fallbacks are wired
/// through clap so `None` here means "neither the flag nor the env var was
/// set" — the plugin layer then picks the per-role default from
/// `crate::app::paths`.
#[derive(Args, Debug, Clone)]
pub struct SharedDataPathArgs {
    /// Override the world-snapshot save location.
    #[arg(long, value_name = "PATH", env = "MUD2_SAVE_PATH")]
    pub save_path: Option<PathBuf>,

    /// Override the accounts database location.
    #[arg(long, value_name = "PATH", env = "MUD2_DB_PATH")]
    pub db_path: Option<PathBuf>,
}

/// TLS flags shared by both binaries.
///
/// On `mud2` only one of `server_tls` / `client_tls` is honoured depending on
/// the resolved runtime (see [`mud2_into_plugin`]); on `server` the server-side
/// fields apply unconditionally.
#[derive(Args, Debug, Clone)]
pub struct SharedTlsArgs {
    /// Enable TLS for the active role (server-side in headless-server mode,
    /// client-side when connecting). On the `server` binary, always enables
    /// server TLS.
    #[arg(long)]
    pub tls: bool,

    /// TLS certificate path (PEM).
    #[arg(long, value_name = "PATH", default_value = "cert.pem")]
    pub tls_cert: PathBuf,

    /// TLS key path (PEM).
    #[arg(long, value_name = "PATH", default_value = "key.pem")]
    pub tls_key: PathBuf,

    /// If cert/key are missing, generate a self-signed pair. Implies `--tls`.
    /// Requires the `dev-self-signed` Cargo feature at build time.
    #[arg(long)]
    pub generate_cert: bool,
}

/// Admin Python REPL UNIX-socket flags shared by both binaries.
///
/// `--admin-socket` accepts an optional path: present alone uses the per-role
/// default, present with a value pins to that path. The `MUD2_ADMIN_SOCKET`
/// env var is honoured both when the flag is absent (clap fills the value
/// slot) and — as a path override — when the flag is bare; see
/// [`Self::resolved`].
#[derive(Args, Debug, Clone)]
pub struct SharedAdminSocketArgs {
    /// Bind the admin Python REPL UNIX socket. Pass alone for the per-role
    /// default path, or with an explicit path: `--admin-socket /tmp/x.sock`.
    /// Honoured only in headless-server mode (a no-op for `--client` /
    /// `--tcp-client`).
    //
    // Why `String` and a sentinel rather than `Option<PathBuf>`:
    // - `num_args = 0..=1` is needed for the optional value, but it forces
    //   `default_missing_value` to be a string clap can parse via the field's
    //   FromStr impl. `PathBuf::from("")` succeeds but the empty path is
    //   indistinguishable from "really, give me the default".
    // - The sentinel `__use_default__` is impossible as a real socket path
    //   on macOS/Linux (it would resolve to a relative file `./__use_default__`
    //   under CWD, which is never what `--admin-socket` is asked to do) and
    //   is private to this module; the caller never sees it.
    #[arg(
        long = "admin-socket",
        value_name = "PATH",
        env = "MUD2_ADMIN_SOCKET",
        num_args = 0..=1,
        default_missing_value = ADMIN_SOCKET_DEFAULT_SENTINEL,
    )]
    admin_socket_raw: Option<String>,

    /// Permission bits for the admin socket, in octal (e.g. `600`, `660`).
    //
    // `default_value` (not `default_value_t`) because clap re-feeds the
    // default through the `value_parser`. With `default_value_t = 0o600`,
    // the default would be rendered via `Display` as `"384"` (decimal) and
    // then rejected by `parse_octal_mode` (8 is not a valid octal digit).
    // `default_value = "600"` is the octal string the parser expects.
    #[arg(
        long,
        value_name = "OCTAL",
        value_parser = crate::app::cli::octal::parse_octal_mode,
        default_value = "600",
    )]
    pub admin_socket_mode: u32,
}

impl SharedAdminSocketArgs {
    /// `(enabled, explicit_path)` for the resolved admin socket.
    ///
    /// - `(false, None)` — flag absent, env var absent.
    /// - `(true, None)` — flag present without a value AND `MUD2_ADMIN_SOCKET`
    ///   unset; caller falls back to the per-role default.
    /// - `(true, Some(path))` — explicit path from the flag or the env var.
    ///   When the flag is bare but `MUD2_ADMIN_SOCKET` is set, the env var
    ///   is honoured as the path (this preserves the pre-clap behaviour
    ///   in `src/main.rs:157` and `src/bin/server.rs:152` where the env var
    ///   was consulted as a path fallback even when the flag was bare).
    pub fn resolved(&self) -> (bool, Option<PathBuf>) {
        match self.admin_socket_raw.as_deref() {
            None => (false, None),
            Some(ADMIN_SOCKET_DEFAULT_SENTINEL) => {
                let env_path = std::env::var("MUD2_ADMIN_SOCKET").ok().map(PathBuf::from);
                (true, env_path)
            }
            Some(p) => (true, Some(PathBuf::from(p))),
        }
    }
}

/// `mud2`-only runtime selector. Marked as a clap `ArgGroup` so the three
/// flags are mutually exclusive — today's hand-rolled parser was last-write-
/// wins, which silently ignored conflicting flags; clap turns the conflict
/// into a clear error.
#[derive(Args, Debug)]
#[group(multiple = false)]
pub struct ModeArgs {
    /// Run as headless TCP server (no GUI).
    #[arg(long, visible_alias = "headless-server")]
    pub server: bool,
    /// Run as a TCP client connecting to a remote server. Implied by `--connect`.
    #[arg(long)]
    pub tcp_client: bool,
    /// Run as embedded client (server + client in one process). This is the default.
    #[arg(long)]
    pub client: bool,
}

// ---------------------------------------------------------------------------
// Top-level parsers.
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(
    name = "mud2",
    version,
    about = "Mud 2.0 — embedded client, TCP client, or headless server.",
    long_about = None,
)]
pub struct Mud2Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub mode: ModeArgs,

    /// Connect to a remote server. Accepts `host:port` or `tls://host:port`;
    /// the `tls://` scheme implies `--tls` for the client and switches the
    /// runtime to `--tcp-client`.
    #[arg(long, value_name = "ADDR")]
    pub connect: Option<String>,

    #[command(flatten)]
    pub data: SharedDataPathArgs,

    /// Override the TcpClient asset-sync cache directory.
    #[arg(long, value_name = "PATH", env = "MUD2_ASSET_CACHE")]
    pub asset_cache: Option<PathBuf>,

    #[command(flatten)]
    pub tls: SharedTlsArgs,

    /// Skip TLS certificate verification when connecting. Implies `--tls`.
    /// Client-side only; ignored in headless-server mode. Dev use only.
    #[arg(long)]
    pub insecure: bool,

    #[command(flatten)]
    pub admin: SharedAdminSocketArgs,
}

#[derive(Parser, Debug)]
#[command(
    name = "server",
    version,
    about = "Mud 2.0 headless server.",
    long_about = None,
)]
pub struct ServerCli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Address to bind the listener on (e.g. `0.0.0.0:7000`).
    #[arg(long, value_name = "ADDR", env = "MUD2_SERVER_BIND")]
    pub bind: Option<String>,

    #[command(flatten)]
    pub data: SharedDataPathArgs,

    #[command(flatten)]
    pub tls: SharedTlsArgs,

    #[command(flatten)]
    pub admin: SharedAdminSocketArgs,
}

// ---------------------------------------------------------------------------
// CLI → GameAppPlugin
// ---------------------------------------------------------------------------

/// `tls://host:port` → (`host:port`, true). Any other prefix is returned
/// as-is with `false`. Inlined from the pre-clap `src/main.rs` helper.
fn strip_tls_scheme(addr: String) -> (String, bool) {
    match addr.strip_prefix("tls://") {
        Some(rest) => (rest.to_owned(), true),
        None => (addr, false),
    }
}

/// Resolve the `mud2` runtime from the explicit mode flags and `--connect`.
/// An explicit mode flag always wins; if none is given and `--connect` is
/// set, the runtime is `TcpClient`; otherwise `EmbeddedClient`.
fn resolve_mud2_runtime(mode: &ModeArgs, connect: bool) -> AppRuntime {
    if mode.server {
        AppRuntime::HeadlessServer
    } else if mode.tcp_client {
        AppRuntime::TcpClient
    } else if mode.client {
        AppRuntime::EmbeddedClient
    } else if connect {
        AppRuntime::TcpClient
    } else {
        AppRuntime::EmbeddedClient
    }
}

/// Build a [`GameAppPlugin`] from a parsed [`Mud2Cli`], folding in the
/// scheme-strip / mode-coalesce / role-gate rules.
pub fn mud2_into_plugin(cli: Mud2Cli) -> GameAppPlugin {
    let (server_addr, tls_from_scheme) = match cli.connect {
        Some(a) => {
            let (rest, is_tls) = strip_tls_scheme(a);
            (Some(rest), is_tls)
        }
        None => (None, false),
    };

    let runtime = resolve_mud2_runtime(&cli.mode, server_addr.is_some());

    // `--generate-cert` implies server `--tls`; `--insecure` and `tls://`
    // both imply client `--tls`. Compute the booleans here rather than via
    // clap `requires` so a plain `--tls` (no extras) keeps working.
    let server_tls_enabled = cli.tls.tls || cli.tls.generate_cert;
    let client_tls_enabled = cli.tls.tls || cli.insecure || tls_from_scheme;

    let server_tls =
        (server_tls_enabled && matches!(runtime, AppRuntime::HeadlessServer)).then(|| {
            ServerTlsArgs {
                cert_path: cli.tls.tls_cert.clone(),
                key_path: cli.tls.tls_key.clone(),
                generate_if_missing: cli.tls.generate_cert,
            }
        });

    let client_tls = (client_tls_enabled && matches!(runtime, AppRuntime::TcpClient))
        .then_some(ClientTlsArgs {
            insecure: cli.insecure,
        });

    #[cfg(unix)]
    let admin_socket = {
        let (enabled, explicit_path) = cli.admin.resolved();
        if enabled && matches!(runtime, AppRuntime::HeadlessServer) {
            let socket_path = explicit_path
                .or_else(|| default_admin_socket_path(runtime))
                .unwrap_or_else(|| PathBuf::from("admin.sock"));
            Some(crate::network::AdminListenArgs {
                socket_path,
                mode: cli.admin.admin_socket_mode,
            })
        } else {
            if enabled && !matches!(runtime, AppRuntime::HeadlessServer) {
                eprintln!(
                    "warning: --admin-socket is only honoured in headless-server mode; ignoring"
                );
            }
            None
        }
    };

    GameAppPlugin {
        runtime,
        server_addr,
        bind_addr: None,
        save_path: cli.data.save_path,
        db_path: cli.data.db_path,
        asset_cache_dir: cli.asset_cache,
        server_tls,
        client_tls,
        #[cfg(unix)]
        admin_socket,
    }
}

/// Build a [`GameAppPlugin`] from a parsed [`ServerCli`]. Runtime is always
/// [`AppRuntime::HeadlessServer`]; only the server-side TLS / admin-socket
/// fields apply.
pub fn server_into_plugin(cli: ServerCli) -> GameAppPlugin {
    let runtime = AppRuntime::HeadlessServer;
    let server_tls_enabled = cli.tls.tls || cli.tls.generate_cert;

    let server_tls = server_tls_enabled.then(|| ServerTlsArgs {
        cert_path: cli.tls.tls_cert.clone(),
        key_path: cli.tls.tls_key.clone(),
        generate_if_missing: cli.tls.generate_cert,
    });

    #[cfg(unix)]
    let admin_socket = {
        let (enabled, explicit_path) = cli.admin.resolved();
        if enabled {
            let socket_path = explicit_path
                .or_else(|| default_admin_socket_path(runtime))
                .unwrap_or_else(|| PathBuf::from("admin.sock"));
            Some(crate::network::AdminListenArgs {
                socket_path,
                mode: cli.admin.admin_socket_mode,
            })
        } else {
            None
        }
    };

    GameAppPlugin {
        runtime,
        server_addr: None,
        bind_addr: cli.bind,
        save_path: cli.data.save_path,
        db_path: cli.data.db_path,
        asset_cache_dir: None,
        server_tls,
        client_tls: None,
        #[cfg(unix)]
        admin_socket,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_exits_cleanly() {
        let err = Mud2Cli::try_parse_from(["mud2", "--help"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn version_exits_cleanly() {
        let err = Mud2Cli::try_parse_from(["mud2", "--version"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn server_help_exits_cleanly() {
        let err = ServerCli::try_parse_from(["server", "--help"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn connect_with_tls_scheme_strips_and_enables_client_tls() {
        let cli = Mud2Cli::try_parse_from(["mud2", "--connect", "tls://example.com:7000"]).unwrap();
        let plugin = mud2_into_plugin(cli);
        assert_eq!(plugin.server_addr.as_deref(), Some("example.com:7000"));
        assert!(matches!(plugin.runtime, AppRuntime::TcpClient));
        assert!(plugin.client_tls.is_some());
        assert!(plugin.server_tls.is_none());
    }

    #[test]
    fn connect_without_scheme_does_not_enable_tls() {
        let cli = Mud2Cli::try_parse_from(["mud2", "--connect", "127.0.0.1:7000"]).unwrap();
        let plugin = mud2_into_plugin(cli);
        assert_eq!(plugin.server_addr.as_deref(), Some("127.0.0.1:7000"));
        assert!(matches!(plugin.runtime, AppRuntime::TcpClient));
        assert!(plugin.client_tls.is_none());
    }

    #[test]
    fn mode_flags_are_mutually_exclusive() {
        let result = Mud2Cli::try_parse_from(["mud2", "--server", "--tcp-client"]);
        assert!(result.is_err());
    }

    #[test]
    fn headless_server_alias_works() {
        let cli = Mud2Cli::try_parse_from(["mud2", "--headless-server"]).unwrap();
        let plugin = mud2_into_plugin(cli);
        assert!(matches!(plugin.runtime, AppRuntime::HeadlessServer));
    }

    #[test]
    fn admin_socket_bare_enables_with_no_path() {
        // Use --server so admin-socket survives the runtime gate.
        let cli = Mud2Cli::try_parse_from(["mud2", "--server", "--admin-socket"]).unwrap();
        let (enabled, path) = cli.admin.resolved();
        assert!(enabled);
        // Path is either None (env var unset) or whatever MUD2_ADMIN_SOCKET
        // happened to point at; the test environment may legitimately have
        // that set, so we don't assert on the exact value.
        let _ = path;
    }

    #[test]
    fn admin_socket_with_explicit_path() {
        let cli =
            Mud2Cli::try_parse_from(["mud2", "--server", "--admin-socket", "/tmp/x.sock"]).unwrap();
        let (enabled, path) = cli.admin.resolved();
        assert!(enabled);
        assert_eq!(path.as_deref(), Some(std::path::Path::new("/tmp/x.sock")));
    }

    #[test]
    fn admin_socket_warning_only_in_headless_server_mode() {
        // EmbeddedClient (default) + --admin-socket should produce a plugin
        // with admin_socket = None and warn (warning to stderr — we don't
        // assert on it, only on the absence of the resource).
        let cli = Mud2Cli::try_parse_from(["mud2", "--admin-socket"]).unwrap();
        let plugin = mud2_into_plugin(cli);
        #[cfg(unix)]
        assert!(plugin.admin_socket.is_none());
        let _ = plugin;
    }

    #[test]
    fn admin_socket_set_in_headless_server_mode() {
        let cli = Mud2Cli::try_parse_from([
            "mud2",
            "--server",
            "--admin-socket",
            "/tmp/x.sock",
            "--admin-socket-mode",
            "660",
        ])
        .unwrap();
        let plugin = mud2_into_plugin(cli);
        #[cfg(unix)]
        {
            let args = plugin.admin_socket.expect("admin_socket Some");
            assert_eq!(args.socket_path, std::path::PathBuf::from("/tmp/x.sock"));
            assert_eq!(args.mode, 0o660);
        }
        let _ = plugin;
    }

    #[test]
    fn invalid_octal_mode_is_a_parse_error() {
        let result = Mud2Cli::try_parse_from(["mud2", "--admin-socket-mode", "9zz"]);
        assert!(result.is_err());
    }

    #[test]
    fn equals_form_works() {
        let cli = Mud2Cli::try_parse_from(["mud2", "--connect=10.0.0.1:7000"]).unwrap();
        assert_eq!(cli.connect.as_deref(), Some("10.0.0.1:7000"));
    }

    #[test]
    fn generate_cert_implies_server_tls() {
        let cli = Mud2Cli::try_parse_from(["mud2", "--server", "--generate-cert"]).unwrap();
        let plugin = mud2_into_plugin(cli);
        let tls = plugin.server_tls.expect("server_tls Some");
        assert!(tls.generate_if_missing);
    }

    #[test]
    fn insecure_implies_client_tls() {
        let cli = Mud2Cli::try_parse_from(["mud2", "--tcp-client", "--insecure"]).unwrap();
        let plugin = mud2_into_plugin(cli);
        let tls = plugin.client_tls.expect("client_tls Some");
        assert!(tls.insecure);
    }

    #[test]
    fn server_cli_bind_and_tls() {
        let cli = ServerCli::try_parse_from([
            "server",
            "--bind",
            "0.0.0.0:1234",
            "--tls",
            "--tls-cert",
            "/etc/c.pem",
            "--tls-key",
            "/etc/k.pem",
        ])
        .unwrap();
        let plugin = server_into_plugin(cli);
        assert!(matches!(plugin.runtime, AppRuntime::HeadlessServer));
        assert_eq!(plugin.bind_addr.as_deref(), Some("0.0.0.0:1234"));
        let tls = plugin.server_tls.expect("server_tls Some");
        assert_eq!(tls.cert_path, PathBuf::from("/etc/c.pem"));
        assert_eq!(tls.key_path, PathBuf::from("/etc/k.pem"));
    }

    #[test]
    fn server_cli_rejects_mud2_only_flags() {
        // --connect / --tcp-client / --insecure / --asset-cache don't exist
        // on the server binary; clap should reject them.
        assert!(ServerCli::try_parse_from(["server", "--connect", "x"]).is_err());
        assert!(ServerCli::try_parse_from(["server", "--tcp-client"]).is_err());
        assert!(ServerCli::try_parse_from(["server", "--insecure"]).is_err());
        assert!(ServerCli::try_parse_from(["server", "--asset-cache", "/p"]).is_err());
    }
}

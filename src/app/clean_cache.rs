//! `paths` and `clean-cache` subcommands shared by `mud2` and `server` binaries.
//!
//! Parsed by clap (definitions live below as `Command` / `CleanCacheArgs`,
//! flattened into the per-binary parsers in `crate::app::cli`). The binary
//! `main()` calls `run(cmd, invoker)` when a subcommand was matched, before
//! constructing the Bevy `App`. No Bevy dependency.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Subcommand};

use crate::app::paths::{
    cache_root_dir, client_paths, data_root_dir, embedded_paths, server_paths,
};

/// Which binary is invoking us. Gates what `clean-cache` without `--all` does
/// and how `paths` is labeled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Invoker {
    /// The main `mud2` binary — default mode wipes the client asset cache.
    Mud2,
    /// The standalone `server` binary — default mode is a no-op (server owns
    /// data, not cache); `--all --yes` is required to wipe anything.
    Server,
}

/// Subcommand surface shared by both binaries. Both `Mud2Cli` and `ServerCli`
/// embed this via `#[command(subcommand)]`; per-invoker semantics for
/// `clean-cache` live in `run` below.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Print resolved data and cache paths.
    Paths,
    /// Wipe cached/local data. Default behaviour depends on the binary —
    /// `mud2` wipes the client asset cache; `server` is a no-op without
    /// `--all --yes`.
    #[command(name = "clean-cache")]
    CleanCache(CleanCacheArgs),
}

#[derive(Args, Debug, Default)]
pub struct CleanCacheArgs {
    /// Also wipe persistent data (accounts.db, world snapshots).
    #[arg(long)]
    pub all: bool,
    /// Skip interactive confirmation (required with --all).
    #[arg(long, short = 'y')]
    pub yes: bool,
    /// Print what would be removed without touching the filesystem.
    #[arg(long)]
    pub dry_run: bool,
}

/// Dispatch a parsed subcommand. Returns the exit code the binary should
/// return without constructing a Bevy app.
pub fn run(cmd: Command, invoker: Invoker) -> ExitCode {
    match cmd {
        Command::Paths => print_paths(invoker),
        Command::CleanCache(args) => run_clean_cache(args, invoker),
    }
}

fn print_paths(invoker: Invoker) -> ExitCode {
    let embedded = embedded_paths();
    let server = server_paths();
    let client = client_paths();

    println!("# Mud 2.0 resolved paths ({:?})", invoker);
    println!();
    println!(
        "[embedded]  data root:      {}",
        data_root_dir().join("embedded").display()
    );
    println!(
        "            accounts.db:    {}",
        embedded.accounts_db.display()
    );
    println!(
        "            world snapshot: {}",
        embedded.world_snapshot.display()
    );
    println!();
    println!(
        "[server]    data root:      {}",
        data_root_dir().join("server").display()
    );
    println!(
        "            accounts.db:    {}",
        server.accounts_db.display()
    );
    println!(
        "            world snapshot: {}",
        server.world_snapshot.display()
    );
    println!();
    println!(
        "[client]    cache root:     {}",
        cache_root_dir().join("client").display()
    );
    println!(
        "            asset overlay:  {}",
        client.asset_cache_dir.display()
    );
    ExitCode::SUCCESS
}

fn run_clean_cache(parsed: CleanCacheArgs, invoker: Invoker) -> ExitCode {
    let mut targets: Vec<PathBuf> = Vec::new();
    let client_cache_root = cache_root_dir().join("client");
    let embedded_dir = data_root_dir().join("embedded");
    let server_dir = data_root_dir().join("server");

    match invoker {
        Invoker::Mud2 => {
            targets.push(client_cache_root.clone());
            if parsed.all {
                targets.push(embedded_dir.clone());
                targets.push(server_dir.clone());
            }
        }
        Invoker::Server => {
            if parsed.all {
                targets.push(server_dir.clone());
                // Server intentionally does not touch client cache or embedded
                // data unless the operator really means --all on the server
                // binary; in practice anyone running the server binary
                // probably wants only the server subtree gone.
            } else {
                println!(
                    "server has no cache to clean; data lives under {}",
                    server_dir.display()
                );
                println!("rerun with --all --yes to wipe server data.");
                return ExitCode::SUCCESS;
            }
        }
    }

    println!("would remove:");
    for t in &targets {
        let marker = if t.exists() { "exists" } else { "missing" };
        println!("  {} [{marker}]", t.display());
    }

    if parsed.dry_run {
        return ExitCode::SUCCESS;
    }

    let wipes_data = parsed.all;
    if wipes_data && !parsed.yes {
        eprintln!();
        eprintln!("--all will delete account databases and world snapshots.");
        eprintln!("rerun with --all --yes to confirm.");
        return ExitCode::from(1);
    }
    if !parsed.yes {
        // Cache-only wipe still prompts, so a typo in the terminal doesn't
        // nuke state silently.
        print!("delete the above? [y/N] ");
        let _ = io::stdout().flush();
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            return ExitCode::from(1);
        }
        let answer = input.trim().to_ascii_lowercase();
        if answer != "y" && answer != "yes" {
            println!("aborted.");
            return ExitCode::SUCCESS;
        }
    }

    let mut had_error = false;
    for t in &targets {
        match remove_path(t) {
            Ok(true) => println!("removed {}", t.display()),
            Ok(false) => println!("skipped {} (did not exist)", t.display()),
            Err(err) => {
                eprintln!("failed to remove {}: {err}", t.display());
                had_error = true;
            }
        }
    }

    if had_error {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn remove_path(path: &Path) -> io::Result<bool> {
    match std::fs::metadata(path) {
        Ok(meta) => {
            if meta.is_dir() {
                std::fs::remove_dir_all(path)?;
            } else {
                std::fs::remove_file(path)?;
            }
            Ok(true)
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

// ---------------------------------------------------------------------------
// In-app "Clean game state" wipe marker (debug tooling).
//
// The embedded-mode debug menu can't safely delete the world snapshot /
// accounts DB at runtime: the world is re-saved on `AppExit` and the accounts
// DB has a live open handle. Instead the in-app button writes a marker listing
// the files to delete and exits; `main()` consumes the marker *before* building
// the Bevy app, so the deletions run while nothing holds those files open.
// ---------------------------------------------------------------------------

const WIPE_MARKER_FILE: &str = ".wipe-on-next-boot";

/// Fixed location of the wipe marker (in the embedded data dir). Independent of
/// `--db-path` / `--save-path` overrides — the marker *contents* carry the
/// actual resolved paths to delete.
pub fn wipe_marker_path() -> PathBuf {
    data_root_dir().join("embedded").join(WIPE_MARKER_FILE)
}

/// Write the wipe marker listing absolute paths to delete on next boot, one per
/// line. Called by the in-app "Clean game state" button just before exit.
pub fn write_wipe_marker(paths: &[PathBuf]) -> io::Result<()> {
    let marker = wipe_marker_path();
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&marker, body)
}

/// Parse marker contents (one path per line) into the list of paths to delete,
/// skipping blank lines. Pure so it can be unit-tested without the filesystem.
fn parse_wipe_marker(contents: &str) -> Vec<PathBuf> {
    contents
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// True if a wipe marker is present. The marker is written *only* by the
/// in-app "Clean game state" button just before it exits, and any pre-existing
/// marker is consumed at the top of `main()`, so a marker found after the Bevy
/// app has run means a wipe + restart was requested this run.
pub fn wipe_marker_pending() -> bool {
    wipe_marker_path().exists()
}

/// Re-exec the current binary with the same arguments. Used after the
/// "Clean game state" button exits, so the fresh boot (which consumes the wipe
/// marker and performs the deletions) happens automatically instead of
/// requiring a manual relaunch.
///
/// On Unix this replaces the process image (`exec`) and only returns on error;
/// elsewhere it spawns a child and returns, letting `main()` exit normally.
/// Either way, callers should fall through to a normal exit if this returns.
pub fn restart_process() {
    let exe = std::env::current_exe()
        .ok()
        .or_else(|| std::env::args_os().next().map(PathBuf::from));
    let Some(exe) = exe else {
        eprintln!("clean game state: cannot locate current executable; not restarting");
        return;
    };
    let mut command = std::process::Command::new(exe);
    command.args(std::env::args_os().skip(1));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // `exec` only returns if it failed to replace the process image.
        let err = command.exec();
        eprintln!("clean game state: failed to restart: {err}");
    }
    #[cfg(not(unix))]
    {
        if let Err(err) = command.spawn() {
            eprintln!("clean game state: failed to restart: {err}");
        }
    }
}

/// If a wipe marker exists, delete every path it lists, then the marker itself.
/// Call once at process start, before constructing the Bevy app. Returns true
/// if a wipe ran. Errors are logged to stderr but never abort startup.
pub fn consume_wipe_marker() -> bool {
    let marker = wipe_marker_path();
    let Ok(contents) = std::fs::read_to_string(&marker) else {
        return false;
    };
    for path in parse_wipe_marker(&contents) {
        match remove_path(&path) {
            Ok(true) => println!("clean game state: removed {}", path.display()),
            Ok(false) => {}
            Err(err) => eprintln!(
                "clean game state: failed to remove {}: {err}",
                path.display()
            ),
        }
    }
    let _ = remove_path(&marker);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::cli::Mud2Cli;
    use clap::Parser;

    #[test]
    fn wipe_marker_round_trips_paths_and_skips_blanks() {
        let body = "/a/world-state.json\n\n  /b/accounts.db  \n";
        let parsed = parse_wipe_marker(body);
        assert_eq!(
            parsed,
            vec![
                PathBuf::from("/a/world-state.json"),
                PathBuf::from("/b/accounts.db"),
            ]
        );
        // Empty marker yields nothing to delete.
        assert!(parse_wipe_marker("").is_empty());
    }

    #[test]
    fn wipe_marker_path_is_in_embedded_dir() {
        let p = wipe_marker_path();
        assert!(p.to_string_lossy().contains("embedded"));
        assert!(p.ends_with(".wipe-on-next-boot"));
    }

    #[test]
    fn clean_cache_subcommand_accepts_flags() {
        let cli = Mud2Cli::try_parse_from(["mud2", "clean-cache", "--all", "--yes", "--dry-run"])
            .expect("parse");
        let Some(Command::CleanCache(args)) = cli.command else {
            panic!("expected CleanCache subcommand, got {:?}", cli.command);
        };
        assert!(args.all);
        assert!(args.yes);
        assert!(args.dry_run);
    }

    #[test]
    fn clean_cache_short_yes_works() {
        let cli = Mud2Cli::try_parse_from(["mud2", "clean-cache", "-y"]).expect("parse");
        let Some(Command::CleanCache(args)) = cli.command else {
            panic!("expected CleanCache subcommand");
        };
        assert!(args.yes);
        assert!(!args.all);
        assert!(!args.dry_run);
    }

    #[test]
    fn clean_cache_rejects_unknown_flag() {
        let result = Mud2Cli::try_parse_from(["mud2", "clean-cache", "--nope"]);
        assert!(result.is_err());
    }

    #[test]
    fn paths_subcommand_parses() {
        let cli = Mud2Cli::try_parse_from(["mud2", "paths"]).expect("parse");
        assert!(matches!(cli.command, Some(Command::Paths)));
    }

    #[test]
    fn non_subcommand_args_parse_without_subcommand() {
        let cli = Mud2Cli::try_parse_from(["mud2", "--tcp-client"]).expect("parse");
        assert!(cli.command.is_none());
        assert!(cli.mode.tcp_client);
    }
}

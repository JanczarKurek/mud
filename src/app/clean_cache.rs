//! `paths` and `clean-cache` subcommands shared by `mud2` and `server` binaries.
//!
//! Dispatched from `main()` before the Bevy `App` is constructed. Prints
//! resolved paths, optionally deletes cache/data. No Bevy dependency.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

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

/// Parse and dispatch a subcommand from `args`. Returns:
/// - `None` if `args[0]` is not one of our subcommands (caller proceeds with normal startup).
/// - `Some(code)` if we handled a subcommand and the process should exit with `code`.
pub fn dispatch(args: &[String], invoker: Invoker) -> Option<ExitCode> {
    let first = args.first()?.as_str();
    match first {
        "paths" => Some(print_paths(invoker)),
        "clean-cache" => Some(run_clean_cache(&args[1..], invoker)),
        _ => None,
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

#[derive(Default)]
struct CleanArgs {
    all: bool,
    yes: bool,
    dry_run: bool,
}

fn parse_clean_args(args: &[String]) -> Result<CleanArgs, String> {
    let mut out = CleanArgs::default();
    for arg in args {
        match arg.as_str() {
            "--all" => out.all = true,
            "--yes" | "-y" => out.yes = true,
            "--dry-run" => out.dry_run = true,
            other => return Err(format!("unknown argument for clean-cache: {other}")),
        }
    }
    Ok(out)
}

fn run_clean_cache(args: &[String], invoker: Invoker) -> ExitCode {
    let parsed = match parse_clean_args(args) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("{msg}");
            eprintln!();
            eprintln!("usage: clean-cache [--all] [--yes] [--dry-run]");
            return ExitCode::from(2);
        }
    };

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_clean_args_accepts_flags() {
        let args = [
            "--all".to_string(),
            "--yes".to_string(),
            "--dry-run".to_string(),
        ];
        let parsed = parse_clean_args(&args).unwrap();
        assert!(parsed.all);
        assert!(parsed.yes);
        assert!(parsed.dry_run);
    }

    #[test]
    fn parse_clean_args_rejects_unknown() {
        let args = ["--nope".to_string()];
        assert!(parse_clean_args(&args).is_err());
    }

    #[test]
    fn dispatch_returns_none_for_non_subcommand() {
        let args = ["--connect".to_string(), "127.0.0.1:7000".to_string()];
        assert!(dispatch(&args, Invoker::Mud2).is_none());
    }
}

use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::app::AppExit;
use bevy::prelude::*;
use clap::Parser;

use mud2::app::clean_cache::{self, Invoker};
use mud2::app::cli::{server_into_plugin, ServerCli};

/// Set by `sigint_handler` when SIGINT (or SIGTERM) is delivered. Polled each
/// frame by `exit_on_signal_flag`, which writes `AppExit` — that's what
/// `ScheduleRunnerPlugin`'s loop checks, and that's what triggers the
/// `AppExit`-listening shutdown paths (player autosave, world snapshot,
/// admin-socket unlink).
///
/// We install the handler via `libc::sigaction` rather than the `ctrlc` crate
/// because Bevy's `dynamic_linking` feature splits the `bevy_app` crate (and
/// therefore Bevy's `TerminalCtrlCHandlerPlugin` plus its internal ctrlc state)
/// into a dylib whose statics aren't reachable from this binary. With dynamic
/// linking on (the default for `cargo run`), Bevy's plugin silently fails to
/// react to SIGINT.
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

extern "C" fn sigint_handler(_: libc::c_int) {
    SHUTDOWN_REQUESTED.store(true, Ordering::Relaxed);
}

fn install_shutdown_signal_handler() {
    // SAFETY: `sigint_handler` is async-signal-safe — it only performs a
    // relaxed atomic store on a static. Make sure SIGINT/SIGTERM aren't
    // masked in the calling thread (the schedule-runner main thread); on
    // some configurations the inherited mask can swallow them.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::sigaddset(&mut set, libc::SIGTERM);
        libc::pthread_sigmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());

        for signum in [libc::SIGINT, libc::SIGTERM] {
            libc::signal(signum, sigint_handler as *const () as libc::sighandler_t);
        }
    }
}

fn exit_on_signal_flag(mut app_exit: MessageWriter<AppExit>) {
    if SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
        // 130 = 128 + SIGINT; conventional shell exit code for Ctrl-C.
        app_exit.write(AppExit::from_code(130));
    }
}

fn main() -> ExitCode {
    let cli = ServerCli::parse();
    if let Some(cmd) = cli.command {
        return clean_cache::run(cmd, Invoker::Server);
    }

    let exit = App::new()
        .add_plugins(server_into_plugin(cli))
        // Install signal handler in a Startup system so it runs *after* every
        // plugin's `build()` — Bevy's `TerminalCtrlCHandlerPlugin` (and any
        // dylib init under `dynamic_linking`) installs its own SIGINT handler
        // from its build hook, and anything we install before plugin build is
        // clobbered. A Startup system runs once after all plugins are
        // constructed, so our handler wins.
        .add_systems(Startup, |_: Commands| install_shutdown_signal_handler())
        .add_systems(Update, exit_on_signal_flag)
        .run();
    match exit {
        AppExit::Success => ExitCode::SUCCESS,
        AppExit::Error(code) => ExitCode::from(code.get()),
    }
}

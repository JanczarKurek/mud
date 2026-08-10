#!/usr/bin/env bash
# Launch a multiplayer test scenario: one headless server plus N TCP clients,
# each auto-logged-in as its own account (player1..playerN) via the --auto-*
# autopilot flags, all in one tmux session so every process log stays visible.
#
# Usage:
#   scripts/multiplayer_test.sh [N] [options]
#
#   N                number of clients (default 2)
#   --port PORT      server port                 (default 7777)
#   --data DIR       scratch data dir            (default /tmp/mud2-mptest)
#   --keep           reuse the data dir instead of wiping it (accounts,
#                    characters, and world state persist between runs)
#   --session NAME   tmux session name           (default mud2test)
#   --release        build/run with --release
#
# The data dir holds the server's accounts DB + world snapshot and a per-client
# asset cache, so the run never touches your real ~/.local/share/mud2 data.
# Client i logs in as playerN with password "testpass" and a same-named
# character (classes cycle fighter/wizard/cleric/vagabond).
#
# Detach with `C-b d`; kill everything with `tmux kill-session -t mud2test`.
set -euo pipefail

cd "$(dirname "$0")/.."

# tmux lives in shell.nix; re-enter through nix-shell (once) if it's missing.
# Must happen before arg parsing shifts "$@" away.
if ! command -v tmux >/dev/null 2>&1; then
  if command -v nix-shell >/dev/null 2>&1 && [[ -z "${MUD2_MPTEST_REEXEC:-}" ]]; then
    export MUD2_MPTEST_REEXEC=1
    exec nix-shell --run "$(printf '%q ' "$0" "$@")"
  fi
  echo "error: tmux not found — enter nix-shell first (tmux is in shell.nix)" >&2
  exit 1
fi

CLIENTS=2
PORT=7777
DATA_DIR=/tmp/mud2-mptest
SESSION=mud2test
KEEP=0
PROFILE_FLAG=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --port) PORT="$2"; shift 2 ;;
    --data) DATA_DIR="$2"; shift 2 ;;
    --keep) KEEP=1; shift ;;
    --session) SESSION="$2"; shift 2 ;;
    --release) PROFILE_FLAG="--release"; shift ;;
    -h|--help) sed -n '2,21p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    [0-9]*) CLIENTS="$1"; shift ;;
    *) echo "unknown argument: $1 (see --help)" >&2; exit 2 ;;
  esac
done

# cargo lives inside nix-shell; wrap every cargo invocation so the script
# works from a bare shell too. Panes must also run inside nix-shell for
# LD_LIBRARY_PATH (wayland/vulkan/alsa).
in_shell() {
  if command -v cargo >/dev/null 2>&1; then
    bash -c "$1"
  else
    nix-shell --run "$1"
  fi
}
pane_cmd() {
  if command -v cargo >/dev/null 2>&1; then
    printf '%s' "$1"
  else
    printf 'nix-shell --run %q' "$1"
  fi
}

echo "==> building server + client binaries"
in_shell "cargo build $PROFILE_FLAG --bin server --bin mud2"

if [[ "$KEEP" -eq 0 ]]; then
  rm -rf "$DATA_DIR"
fi
mkdir -p "$DATA_DIR"

tmux kill-session -t "$SESSION" 2>/dev/null || true

SERVER_CMD="cargo run $PROFILE_FLAG --bin server -- \
  --bind 127.0.0.1:$PORT \
  --db-path $DATA_DIR/accounts.db \
  --save-path $DATA_DIR/world-state.json"

echo "==> starting tmux session '$SESSION' (server on 127.0.0.1:$PORT, $CLIENTS clients)"
tmux new-session -d -s "$SESSION" -n game "$(pane_cmd "$SERVER_CMD")"
# Keep dead panes around so crash logs stay readable (respawn with C-b r if
# bound, or `tmux respawn-pane -t ...`).
tmux set-option -t "$SESSION" -w remain-on-exit on
tmux select-pane -t "$SESSION:game.0" -T server

# Wait for the listener before spawning clients — the autopilot retries
# anyway, this just keeps the client logs free of connect noise.
echo -n "==> waiting for server"
for _ in $(seq 1 120); do
  if (exec 3<>"/dev/tcp/127.0.0.1/$PORT") 2>/dev/null; then
    exec 3>&- 3<&- || true
    break
  fi
  echo -n .
  sleep 1
done
echo

CLASSES=(fighter wizard cleric vagabond)
for i in $(seq 1 "$CLIENTS"); do
  CLASS="${CLASSES[$(((i - 1) % ${#CLASSES[@]}))]}"
  CLIENT_CMD="cargo run $PROFILE_FLAG --bin mud2 -- \
    --connect 127.0.0.1:$PORT \
    --auto-login player$i \
    --auto-class $CLASS \
    --asset-cache $DATA_DIR/client$i-assets"
  tmux split-window -t "$SESSION:game" "$(pane_cmd "$CLIENT_CMD")"
  tmux select-pane -t "$SESSION:game" -T "player$i"
  tmux select-layout -t "$SESSION:game" tiled
done

tmux set-option -t "$SESSION" -w pane-border-status top
tmux select-pane -t "$SESSION:game.0"

if [[ -n "${TMUX:-}" ]]; then
  tmux switch-client -t "$SESSION"
else
  tmux attach-session -t "$SESSION"
fi

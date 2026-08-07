#!/usr/bin/env bash
# Build a distributable Windows (x86_64) zip of the mud2 client.
#
# Run from inside the project's nix-shell — `shell.nix` provides the
# mingw-w64 cross toolchain, the CARGO_TARGET_X86_64_PC_WINDOWS_GNU_* env
# vars, zip, and rsync. The script does NOT fetch anything from the network;
# if a tool is missing, fix shell.nix.
#
# Output: target/packaging/mud2-win64.zip  (mud2-win64/mud2.exe + assets/)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

OUT_DIR="$REPO_ROOT/target/packaging"
STAGE="$OUT_DIR/mud2-win64"
TARGET=x86_64-pc-windows-gnu

mkdir -p "$OUT_DIR"

require() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "error: '$1' not on PATH — run from inside the project nix-shell (\`nix-shell\` at repo root)." >&2
        exit 1
    fi
}
require x86_64-w64-mingw32-gcc
require x86_64-w64-mingw32-objdump
require zip
require rsync

# A set $RUSTFLAGS — even an empty one — makes cargo skip every target-scoped
# rustflags source ([target.*] in .cargo/config.toml AND the
# CARGO_TARGET_*_RUSTFLAGS vars from shell.nix), which breaks the winpthread
# link. Shells opened before shell.nix dropped its `RUSTFLAGS = []` template
# leftover still export it; scrub it here.
if [[ -n "${RUSTFLAGS+x}" ]]; then
    if [[ -n "$RUSTFLAGS" ]]; then
        echo "warning: unsetting RUSTFLAGS='$RUSTFLAGS' — it would mask the target-scoped cross flags from shell.nix" >&2
    fi
    unset RUSTFLAGS
fi

echo "==> Building $TARGET dist binary"
# --no-default-features drops `dynamic_linking` (no bevy dylib at runtime)
# AND — deliberately — `server-sim`/`editor`: the shipped zip is the thin
# online-only client (TcpClient mode, title-screen server picker; no embedded
# single-player, no map editor). For an offline-capable build add
# `--features editor`. `--profile dist` carries the packaging-only
# lto/codegen-units/strip settings (see Cargo.toml).
#
# No dedicated CARGO_TARGET_DIR (unlike build-appimage.sh): --target already
# writes to the disjoint target/x86_64-pc-windows-gnu/dist/, and the windows
# rustflags are target-scoped env vars, so host build fingerprints don't churn.
cargo build --profile dist --bin mud2 --no-default-features --target "$TARGET"

BIN="$REPO_ROOT/target/$TARGET/dist/mud2.exe"
if [[ ! -f "$BIN" ]]; then
    echo "error: $BIN missing after cargo build" >&2
    exit 1
fi

echo "==> Staging $STAGE"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp "$BIN" "$STAGE/mud2.exe"

# assets/ goes next to the exe: Bevy's release-mode AssetServer resolves
# paths from current_exe().parent(), not CWD (same layout as the AppImage).
echo "==> Copying assets/"
rsync -a --delete "$REPO_ROOT/assets/" "$STAGE/assets/"

echo "==> Checking DLL imports"
# Anything from the mingw runtime must ship beside the exe; everything else
# (KERNEL32, WS2_32, bcrypt, dwmapi, ...) is assumed to be a system DLL.
# -static-libgcc in the shell.nix rustflags should keep libgcc out; the
# case arms exist so a regression fails loudly instead of shipping a zip
# that can't start on a clean Windows box.
mapfile -t dlls < <(x86_64-w64-mingw32-objdump -p "$STAGE/mud2.exe" \
    | awk '/DLL Name:/ {print $3}')
printf '  imports: %s\n' "${dlls[*]}"
for dll in "${dlls[@]}"; do
    case "$dll" in
        libwinpthread-1.dll|libgcc_s_seh-1.dll|libstdc++-6.dll)
            src="${MUD2_MINGW_DLL_DIR:-}/$dll"
            if [[ -f "$src" ]]; then
                cp "$src" "$STAGE/"
                echo "  bundled mingw runtime DLL: $dll"
            else
                echo "error: exe imports $dll but it isn't in MUD2_MINGW_DLL_DIR='${MUD2_MINGW_DLL_DIR:-}'" >&2
                echo "       (set by shell.nix — run from inside the project nix-shell)" >&2
                exit 1
            fi
            ;;
        *) : ;;
    esac
done

echo "==> Building zip"
rm -f "$OUT_DIR/mud2-win64.zip"
(cd "$OUT_DIR" && zip -qr mud2-win64.zip mud2-win64)

echo
echo "Built: $OUT_DIR/mud2-win64.zip"
ls -lh "$OUT_DIR/mud2-win64.zip"

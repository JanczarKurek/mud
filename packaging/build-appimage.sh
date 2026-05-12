#!/usr/bin/env bash
# Build a mud2 AppImage for x86_64 Linux.
#
# Run from inside the project's nix-shell — `shell.nix` provides linuxdeploy,
# appimagetool, rsync, and the Rust toolchain. The script does NOT fetch
# anything from the network; if a tool is missing, fix shell.nix.
#
# Output: target/packaging/Mud_2.0-x86_64.AppImage
#
# Caveat: the resulting AppImage will refuse to run on glibc older than the
# build host's. Build on the oldest distro you want to support (or a container)
# if you care about reach.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

OUT_DIR="$REPO_ROOT/target/packaging"
APPDIR="$OUT_DIR/AppDir"
ARCH="${ARCH:-x86_64}"

mkdir -p "$OUT_DIR"

require() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "error: '$1' not on PATH — run from inside the project nix-shell (\`nix-shell\` at repo root)." >&2
        exit 1
    fi
}
require mud2-fhs
require linuxdeploy
require appimagetool
require rsync
require patchelf

echo "==> Building release binary inside FHS sandbox (mud2-fhs)"
# Build runs inside `mud2-fhs` (defined in shell.nix via buildFHSEnv) so the
# linker emits /lib64/ld-linux-x86-64.so.2 as PT_INTERP. The regular nix-shell
# would produce a Nix-store-bound binary that can't run on Ubuntu/Fedora/etc.
# --no-default-features drops the `dynamic_linking` Cargo feature so the
# binary doesn't need libbevy_dylib.so at runtime.
#
# Use a dedicated CARGO_TARGET_DIR so the FHS-built artifacts don't churn
# against the dev `cargo run --bin mud2` cache (different RUSTFLAGS would
# otherwise trigger a relink every time you switch contexts).
export CARGO_TARGET_DIR="$REPO_ROOT/target/fhs"
mud2-fhs -c 'cargo build --release --bin mud2 --no-default-features'

BIN="$CARGO_TARGET_DIR/release/mud2"
if [[ ! -x "$BIN" ]]; then
    echo "error: $BIN missing after cargo build" >&2
    exit 1
fi

interp=$(patchelf --print-interpreter "$BIN")
echo "  binary interpreter: $interp"
if [[ "$interp" != /lib64/* && "$interp" != /lib/* ]]; then
    echo "error: interpreter is '$interp' — expected a /lib64 or /lib FHS path." >&2
    echo "       The cargo build leaked outside the FHS sandbox; check shell.nix mud2Fhs." >&2
    exit 1
fi

# Some Nix-built crates (especially anything using build.rs to query system
# pkg-config) can still leak /nix/store paths into the binary's RUNPATH even
# inside the FHS sandbox. Strip those — linuxdeploy will set its own
# $ORIGIN/../lib rpath in the staged copy anyway.
runpath=$(patchelf --print-rpath "$BIN" 2>/dev/null || true)
if echo "$runpath" | grep -q '/nix/store'; then
    # `grep -v` returns 1 when every line matches the inverted pattern (i.e.
    # the RUNPATH is entirely /nix/store entries) — with pipefail that would
    # kill the script via set -e. The `|| true` keeps the substitution alive
    # for the empty-result case, which the next branch handles explicitly.
    cleaned=$(printf '%s' "$runpath" | tr ':' '\n' | { grep -v '^/nix/store' || true; } | paste -sd:)
    if [[ -z "$cleaned" ]]; then
        patchelf --remove-rpath "$BIN"
        echo "  removed /nix/store-only RUNPATH"
    else
        patchelf --set-rpath "$cleaned" "$BIN"
        echo "  cleaned RUNPATH to: $cleaned"
    fi
fi

echo "==> Staging AppDir at $APPDIR"
# Only pre-stage what linuxdeploy doesn't manage: the binary and the assets.
# linuxdeploy populates AppRun / desktop / icon from --custom-apprun /
# --desktop-file / --icon-file sources — don't copy those in advance or it'll
# try to copy a file onto itself and bail.
#
# assets/ goes next to the binary (usr/bin/assets/) because Bevy's release-mode
# AssetServer resolves paths from current_exe().parent(), not CWD.
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"
cp "$BIN" "$APPDIR/usr/bin/mud2"

echo "==> Copying assets/"
rsync -a --delete "$REPO_ROOT/assets/" "$APPDIR/usr/bin/assets/"

echo "==> Bundling shared libraries via linuxdeploy"
# --custom-apprun keeps our AppRun (which `cd`s into the assets dir).
# linuxdeploy walks the binary's needed-libs and copies what isn't on its
# built-in excludelist (libvulkan, libGL, libwayland-*, libX*, libasound, etc.
# stay host-provided — bundling them tends to fight the host driver).
NO_STRIP=1 linuxdeploy \
    --appdir "$APPDIR" \
    --executable "$APPDIR/usr/bin/mud2" \
    --custom-apprun "$REPO_ROOT/packaging/appimage/AppRun" \
    --desktop-file "$REPO_ROOT/packaging/appimage/mud2.desktop" \
    --icon-file "$REPO_ROOT/packaging/appimage/mud2.png"

echo "==> Building AppImage"
cd "$OUT_DIR"
appimagetool "$APPDIR" "Mud_2.0-${ARCH}.AppImage"

echo
echo "Built: $OUT_DIR/Mud_2.0-${ARCH}.AppImage"
ls -lh "$OUT_DIR/Mud_2.0-${ARCH}.AppImage"

# TBD Mud

This is partially an exercise in workin with codex and in the future maybe a cool Tibia-style engine for making small
graphical MUDs.

## Disclaimer
This is basically almost entirely vibe-coded with some rather mild
architecture-level steering. That means, the ideas for the systems
and the development directions is for now human-based.

All the assets for now are AI-generated placeholders, mostly
not the bad type of ai generated art but ya know, codex writing
pixelart with imagemagick by hand.

Given the fact that I have no idea how most of this code works,
I give zero warranty whatsoever about whether running this code
is a security risk.

## What's here
Very much work in progress. For now things that work:
- Movement (with diagonals) and grid-based collision
- Equipment, containers, pouches, currency (copper/silver/gold) and carry weight
- Combat with classes, XP/levels, partial-drop death penalty
- Magic system with class-gated spells and level-scaled caster mana
- Crafting basics with a recipe book
- Vendors / trading
- Persistent world with multi-space portals (overworld, underworld, ephemeral dungeons)
- Multiplayer over plain TCP or TLS, with account login (sqlite + Argon2)
- Character creation with a class picker
- Dialogs (yarnspinner) + python-scripted questing with a docked quest log
- In-app map editor with placement, modal property editing, undo, and YAML serialization

## How to run

`cargo run --bin mud2`

or if you wish to run the standalone server then

`cargo run --bin server` and then
`cargo run --bin mud2 -- --connect 127.0.0.1:7000`.

## Packaging for distribution (Linux AppImage)

To produce a single-file build that runs on most modern Linux distros without
a Rust toolchain (run from inside the project's `nix-shell` — that's where
`linuxdeploy` and `appimagetool` come from):

```
nix-shell
bash packaging/build-appimage.sh
```

Output: `target/packaging/Mud_2.0-x86_64.AppImage`. Make it executable and
double-click, or run from a terminal. Saves still land in
`~/.local/share/mud2/embedded/`.

The build:
- Compiles in release mode with Bevy's `dynamic_linking` feature **off**
  (single self-contained binary).
- Runs cargo inside `mud2-fhs` (a `buildFHSEnv` sandbox defined in shell.nix)
  so the binary's dynamic linker is `/lib64/ld-linux-x86-64.so.2` — the FHS
  path that exists on Ubuntu/Fedora/Arch/SteamOS but NOT on NixOS.
- Statically links sqlite3 via `libsqlite3-sys/bundled`.
- Bundles `assets/` and host-portable shared libs (libxkbcommon, libffi, etc.)
  via `linuxdeploy`. Vulkan, OpenGL, Wayland/X11, and ALSA come from the host.

Testing on NixOS: the AppImage's interpreter is `/lib64/ld-linux-x86-64.so.2`
(by design), so it won't execute directly. Use `steam-run
./target/packaging/Mud_2.0-x86_64.AppImage` or set up `programs.nix-ld`. On
non-Nix distros, just double-click.

Glibc note: the AppImage refuses to run on glibc older than the build host's.
The FHS sandbox uses nixpkgs glibc (currently 2.40), so the AppImage needs
glibc 2.40+ on the target. For broader reach, swap `pkgs.buildFHSEnv` for a
docker-based build against an older glibc.



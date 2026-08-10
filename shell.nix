{ pkgs ? import <nixpkgs> {} }:
  let
    overrides = (builtins.fromTOML (builtins.readFile ./rust-toolchain.toml));
    # NOTE: pass the plain packages here, never `.dev` outputs — makeLibraryPath
    # keeps an explicitly-selected output as-is, and dev outputs' lib/ holds
    # only pkgconfig/cmake files (no .so), which breaks the game at runtime
    # ("error while loading shared libraries: libudev.so.1"). The .dev outputs
    # belong in buildInputs / PKG_CONFIG_PATH below.
    libPath = with pkgs; lib.makeLibraryPath [
      pkgs.alsa-lib
      pkgs.systemd # libudev.so.1
      pkgs.wayland
      pkgs.libxkbcommon
      pkgs.libffi
      pkgs.expat
      pkgs.vulkan-loader
      # load external libraries that you need in your rust project here
    ];
    lib = pkgs.lib;

    # AppImage tooling for `packaging/build-appimage.sh`. Neither linuxdeploy
    # nor appimagetool ships in nixpkgs, so we pull the upstream AppImages and
    # wrap them with `appimageTools.wrapType2` (sets up the FHS environment
    # each AppImage internally expects). Hashes pin the binaries — bump if
    # upstream `continuous` rebuilds and the build script fails with a hash
    # mismatch.
    linuxdeploy = pkgs.appimageTools.wrapType2 {
      pname = "linuxdeploy";
      version = "1-alpha-20251107-1";
      src = pkgs.fetchurl {
        url = "https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-x86_64.AppImage";
        sha256 = "c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d";
      };
    };
    appimagetool = pkgs.appimageTools.wrapType2 {
      pname = "appimagetool";
      version = "continuous";
      src = pkgs.fetchurl {
        url = "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage";
        sha256 = "1q0kkp5r0a281b4m1afabz7y11c9cmjd2yn32s6qwvyndhmixmx6";
      };
    };

    # Windows cross toolchain for packaging/build-windows.sh.
    # pkgsCross.mingwW64 = x86_64-w64-mingw32 (gcc 14.x, posix thread model —
    # hence the winpthreads lib below). First `nix-shell` after adding this
    # pulls a sizable closure and may compile gcc if the channel snapshot
    # isn't hydra-cached; subsequent entries are instant.
    mingwCC = pkgs.pkgsCross.mingwW64.stdenv.cc;
    winPthreads = pkgs.pkgsCross.mingwW64.windows.pthreads;

    # FHS sandbox for producing portable (non-Nix) Linux binaries. Inside this
    # env, /lib64/ld-linux-x86-64.so.2 exists and pkg-config / linkers resolve
    # libs at FHS paths instead of /nix/store. Combined with the RUSTFLAGS
    # below, the resulting `mud2` binary's PT_INTERP points at the FHS dynamic
    # linker, so the AppImage runs on Ubuntu/Fedora/Arch/SteamOS hosts.
    #
    # Used by `packaging/build-appimage.sh`. The regular nix-shell is still the
    # right place for `cargo run --bin mud2` during dev — those binaries stay
    # Nix-linked on purpose.
    mud2Fhs = pkgs.buildFHSEnv {
      name = "mud2-fhs";
      targetPkgs = pkgs: with pkgs; [
        rustup
        gcc
        clang
        mold
        pkg-config
        # X11
        xorg.libX11
        xorg.libXcursor
        xorg.libXrandr
        xorg.libXi
        xorg.libxcb
        # Wayland
        wayland
        libxkbcommon
        # Graphics
        vulkan-loader
        libGL
        # Audio
        alsa-lib
        # Misc deps pulled in by Bevy / rustpython / yarnspinner
        libffi
        expat
        systemd
        zlib
        bashInteractive
        coreutils
        findutils
        gnused
        gnugrep
        gawk
      ];
      profile = ''
        export RUSTC_VERSION="${overrides.toolchain.channel}"
        export PATH="$PATH:''${CARGO_HOME:-$HOME/.cargo}/bin"
        export PATH="$PATH:''${RUSTUP_HOME:-$HOME/.rustup}/toolchains/$RUSTC_VERSION-x86_64-unknown-linux-gnu/bin"
        # Force the linker to bake /lib64/ld-linux-x86-64.so.2 into the binary
        # (the FHS path that exists on every non-Nix distro). Without this,
        # rustc/gcc pick up the Nix-store interpreter and the AppImage won't
        # execute outside this sandbox.
        export RUSTFLAGS="-C link-arg=-Wl,--dynamic-linker=/lib64/ld-linux-x86-64.so.2"
      '';
      runScript = "bash";
    };
in
  pkgs.mkShell rec {
    buildInputs = with pkgs; [
      clang
      mold
      # Replace llvmPackages with llvmPackages_X, where X is the latest LLVM version (at the time of writing, 16)
      llvmPackages.bintools
      pkg-config
      rustup
      python313Packages.ipython
      python313Packages.pillow
      python313Packages.numpy
      ripgrep
      tmux # scripts/multiplayer_test.sh — server + N clients in one session
      pkgs.alsa-lib.dev
      pkgs.systemd.dev
      pkgs.wayland.dev
      pkgs.vulkan-loader
      # Packaging tooling — used by packaging/build-appimage.sh.
      linuxdeploy
      appimagetool
      mud2Fhs
      pkgs.patchelf
      pkgs.rsync
    ];
    # nativeBuildInputs (NOT buildInputs) for the mingw cross compiler: the
    # cc-wrapper setup hook exports plain $CC for host-role compilers, which
    # would hijack cc-rs for native builds of libsqlite3-sys/ring/etc. In the
    # build role it only exports the harmless CC_FOR_BUILD. All binaries are
    # prefixed x86_64-w64-mingw32-* so no PATH shadowing either way.
    nativeBuildInputs = [
      mingwCC
      mingwCC.bintools # x86_64-w64-mingw32-objdump/ar for the DLL-import scan
      pkgs.zip         # packaging/build-windows.sh
    ];

    # Windows cross-compile knobs, target-scoped so host builds are untouched.
    # Kept here (absolute store paths) rather than .cargo/config.toml — and
    # note CARGO_TARGET_*_RUSTFLAGS env would fully OVERRIDE any config-file
    # rustflags for the target, so keep all windows rustflags in this one spot.
    CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = "${mingwCC}/bin/x86_64-w64-mingw32-gcc";
    CC_x86_64_pc_windows_gnu = "${mingwCC}/bin/x86_64-w64-mingw32-gcc";
    CXX_x86_64_pc_windows_gnu = "${mingwCC}/bin/x86_64-w64-mingw32-g++";
    AR_x86_64_pc_windows_gnu = "${mingwCC.bintools}/bin/x86_64-w64-mingw32-ar";
    # -L winpthreads: gcc's posix thread model needs it at link time.
    # -static-libgcc: drops the libgcc_s_seh-1.dll runtime dependency.
    CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS =
      "-L native=${winPthreads}/lib -C link-arg=-static-libgcc";
    # Where build-windows.sh copies mingw runtime DLLs from if the exe still
    # imports any (libwinpthread-1.dll et al).
    MUD2_MINGW_DLL_DIR = "${winPthreads}/bin";

    RUSTC_VERSION = overrides.toolchain.channel;
    # https://github.com/rust-lang/rust-bindgen#environment-variables
    LIBCLANG_PATH = pkgs.lib.makeLibraryPath [ pkgs.llvmPackages_latest.libclang.lib ];
    shellHook = ''
      export PATH=$PATH:''${CARGO_HOME:-~/.cargo}/bin
      export PATH=$PATH:''${RUSTUP_HOME:-~/.rustup}/toolchains/$RUSTC_VERSION-x86_64-unknown-linux-gnu/bin/
      '';
    # NOTE: do NOT set a RUSTFLAGS env var here, even an empty one. Cargo
    # treats a set-but-empty $RUSTFLAGS as authoritative and then SKIPS every
    # target-scoped rustflags source (.cargo/config.toml [target.*] rustflags
    # AND the CARGO_TARGET_*_RUSTFLAGS vars above). The template's empty
    # `RUSTFLAGS = []` silently disabled mold on host builds and broke the
    # Windows cross link. Add per-target flags via CARGO_TARGET_*_RUSTFLAGS
    # or .cargo/config.toml instead.
    PKG_CONFIG_PATH = lib.makeSearchPath ''lib/pkgconfig'' [
      pkgs.systemd.dev
      pkgs.alsa-lib.dev
      pkgs.wayland.dev
      pkgs.libxkbcommon.dev
      pkgs.libffi.dev
      pkgs.expat.dev
    ];
    LD_LIBRARY_PATH = libPath;
    # Add glibc, clang, glib, and other headers to bindgen search path
    BINDGEN_EXTRA_CLANG_ARGS =
    # Includes normal include path
    (builtins.map (a: ''-I"${a}/include"'') [
      # add dev libraries here (e.g. pkgs.libvmi.dev)
      pkgs.glibc.dev 
      pkgs.alsa-lib.dev
      pkgs.systemd.dev
      pkgs.wayland.dev
    ])
    # Includes with special directory paths
    ++ [
      ''-I"${pkgs.llvmPackages_latest.libclang.lib}/lib/clang/${pkgs.llvmPackages_latest.libclang.version}/include"''
      ''-I"${pkgs.glib.dev}/include/glib-2.0"''
      ''-I${pkgs.glib.out}/lib/glib-2.0/include/''
    ];
  }

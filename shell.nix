{ pkgs ? import <nixpkgs> {} }:
  let
    overrides = (builtins.fromTOML (builtins.readFile ./rust-toolchain.toml));
    libPath = with pkgs; lib.makeLibraryPath [
      pkgs.alsa-lib.dev
      pkgs.systemd.dev
      pkgs.wayland
      pkgs.libxkbcommon
      pkgs.libffi
      pkgs.expat.dev
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
      version = "continuous";
      src = pkgs.fetchurl {
        url = "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage";
        sha256 = "103wbx3wi4srj9yxfsqx068qjkvzmyxk51iwrkhm7lhm8ycnmv8x";
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
    RUSTC_VERSION = overrides.toolchain.channel;
    # https://github.com/rust-lang/rust-bindgen#environment-variables
    LIBCLANG_PATH = pkgs.lib.makeLibraryPath [ pkgs.llvmPackages_latest.libclang.lib ];
    shellHook = ''
      export PATH=$PATH:''${CARGO_HOME:-~/.cargo}/bin
      export PATH=$PATH:''${RUSTUP_HOME:-~/.rustup}/toolchains/$RUSTC_VERSION-x86_64-unknown-linux-gnu/bin/
      '';
    # Add precompiled library to rustc search path
    RUSTFLAGS = (builtins.map (a: ''-L ${a}/lib'') [
      # add libraries here (e.g. pkgs.libvmi)
    ]);
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

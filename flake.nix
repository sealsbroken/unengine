{
  description = "Un — tutor engine (unengine) and lessons (unmath): one dev shell for both checkouts";

  # The system registry pin: no network round-trip, the same nixpkgs the host runs.
  inputs.nixpkgs.url = "nixpkgs";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };

      # Libraries the engine links against (pkg-config at build time, .so at run time).
      systemLibs = with pkgs; [
        alsa-lib          # cpal: the microphone and the speakers
        udev              # nokhwa: enumerating cameras
        libxkbcommon      # eframe/egui: keyboard through X11 and Wayland
        wayland
        wayland-protocols
        libGL             # eframe glow backend
        libX11
        libXcursor
        libXrandr
        libXi
        libxcb
        gtk3              # named by the engine README for Linux
        openssl           # ort's download-binaries (native-tls)
        zlib
      ];

      # What a built binary needs at run time: sherpa-onnx and onnxruntime are C++,
      # egui loads libGL and libwayland at run time.
      runtimeLibs = with pkgs; [
        alsa-lib
        libGL
        wayland
        libxkbcommon
        libX11
        libXcursor
        libXrandr
        libXi
        libxcb
        openssl
        zlib
        stdenv.cc.cc.lib
      ];

      # The Android SDK the app asks for: compileSdk 36, build-tools 36, NDK 29.0.14206865
      # (tools/android.sh takes the newest NDK under $ANDROID_HOME/ndk). Prebuilt by nixpkgs,
      # so no sdkmanager downloads; the components are unfree and behind nixpkgs' licence
      # switches, hence a second package set rather than flags on the whole shell.
      androidPkgs = import nixpkgs {
        inherit system;
        config = {
          allowUnfree = true;
          android_sdk.accept_license = true;
        };
      };
      androidSdk = androidPkgs.androidenv.composeAndroidPackages {
        platformVersions = [ "36" ];
        # 36 for the project, 35 because AGP 8.11 defaults to it and would otherwise try to
        # install into the read-only store SDK.
        buildToolsVersions = [ "36.0.0" "35.0.0" ];
        includeNDK = true;
        ndkVersion = "29.0.14206865";
        includeEmulator = false;
      };
      androidHome = "${androidSdk.androidsdk}/libexec/android-sdk";
    in {
      devShells.${system} = {
        # The engine and the lessons: build, test, clippy, fmt, the model tools.
        default = pkgs.mkShell {
          name = "un";

          nativeBuildInputs = with pkgs; [
            rustc
            cargo
            rustfmt
            clippy
            pkg-config
            cmake
            clang
            llvmPackages.libclang      # bindgen crates (alsa-sys and friends)
            python3                    # tools/fetch_models.py
            uv                         # tools/label.py, tools/train_detector.py
          ];

          buildInputs = systemLibs;

          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${pkgs.stdenv.cc.libc.dev}/include";

          shellHook = ''
            # Keep build artifacts out of the checkouts and off the other user's target/.
            export CARGO_TARGET_DIR="''${CARGO_TARGET_DIR:-$HOME/.cache/un-target}"
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath runtimeLibs}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

            echo "un: rust $(rustc --version | cut -d' ' -f2), target $CARGO_TARGET_DIR"
            echo "    engine tests:  cargo test --workspace   (in unengine)"
            echo "    lesson tests:  cargo test               (in unmath, same shell)"
            echo "    from unmath:   nix develop ../unengine -c bash -c 'cargo test'"
            echo "    the gate:      bash tools/check.sh   (fmt, clippy and tests: every workspace + unmath)"
          '';
        };

        # The Android build: nixpkgs' Rust cannot gain an Android target, so this shell uses
        # rustup (installed into the user cache on first entry) with `cargo-ndk`, plus Gradle,
        # JDK 17 and zsh for tools/android.sh.
        android = pkgs.mkShell {
          name = "un-android";

          nativeBuildInputs = with pkgs; [
            rustup
            cargo-ndk
            gradle
            jdk17
            zsh
            pkg-config
            cmake
            clang
            llvmPackages.libclang
          ];

          # The SDK plus what the host-side build scripts need (ort downloads its binaries
          # over native-tls, i.e. openssl).
          buildInputs = [ androidSdk.androidsdk pkgs.openssl pkgs.zlib ];

          ANDROID_HOME = androidHome;
          ANDROID_SDK_ROOT = androidHome;
          ANDROID_NDK_HOME = "${androidHome}/ndk/29.0.14206865";
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";

          shellHook = ''
            export CARGO_TARGET_DIR="''${CARGO_TARGET_DIR:-$HOME/.cache/un-target}"
            export RUSTUP_HOME="''${RUSTUP_HOME:-$HOME/.cache/rustup}"
            export CARGO_HOME="''${CARGO_HOME:-$HOME/.cargo}"
            export JAVA_HOME="${pkgs.jdk17}"
            export RUSTUP_TOOLCHAIN=stable

            rustup toolchain list 2>/dev/null | grep -q '^stable' || rustup toolchain install stable --profile minimal
            rustup target list --installed --toolchain stable | grep -qx aarch64-linux-android \
              || rustup target add --toolchain stable aarch64-linux-android

            # rustup ships gcc-ld/ld.lld as a `#!/usr/bin/env bash` script, and NixOS has no
            # /usr/bin/env: clang then dies with "posix_spawn failed: No such file or directory"
            # while linking build scripts. Point those shims at the store bash once.
            for f in "$RUSTUP_HOME"/toolchains/*/lib/rustlib/*/bin/gcc-ld/*; do
              [ -f "$f" ] || continue
              if head -1 "$f" | grep -q '^#!/usr/bin/env'; then
                sed -i "1s|^.*\$|#!${pkgs.bash}/bin/bash|" "$f"
              fi
            done

            echo "un android: SDK $ANDROID_HOME"
            echo "            NDK $ANDROID_NDK_HOME"
            echo "            build: un-aab   (unsigned AAB, logs to ~/.cache/un-gradle.log)"

            # Two things this host needs and the project scripts assume away: the wrapper's
            # `#!/bin/sh` shebang (NixOS has no /bin/sh) and AAPT2, which AGP downloads as a
            # glibc binary that cannot start here — so use the SDK's own patched aapt2.
            un-aab() {
              local here="''${1:-$PWD}"
              ( cd "$here/apps/android" && bash ./gradlew --no-daemon --console=plain \
                  -Pandroid.aapt2FromMavenOverride="$ANDROID_HOME/build-tools/35.0.0/aapt2" \
                  :app:bundleRelease )
            }
            export -f un-aab 2>/dev/null || true
          '';
        };
      };
    };
}

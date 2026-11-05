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

      # What a plain script (not the dev shell) must export: pkg-config for the build, the .so
      # paths for running the window, a CA bundle for ort's and sherpa-rs' downloads.
      nativeEnv = ''
        export LIBCLANG_PATH="${pkgs.llvmPackages.libclang.lib}/lib"
        export PKG_CONFIG_PATH="${pkgs.lib.makeSearchPathOutput "dev" "lib/pkgconfig" systemLibs}"
        export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath runtimeLibs}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
        export SSL_CERT_FILE="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
      '';

      # One command each, working on the checkout they are run from ($PWD), never on the store
      # copy of this flake: in unengine `nix run .#build`, from unmath `nix run ../unengine#build`.
      build = pkgs.writeShellApplication {
        name = "un-build";
        runtimeInputs = with pkgs; [ rustc cargo pkg-config clang cmake git ];
        text = ''
          here="''${1:-$PWD}"
          [ "$#" -gt 0 ] && shift
          [ -f "$here/apps/un/Cargo.toml" ] || { echo "no apps/un/Cargo.toml in $here: run this from the unengine checkout" >&2; exit 2; }
          [ -d "$here/../unmath" ] || { echo "the unmath lessons are missing: clone sealsbroken/unmath next to this repository (../unmath)" >&2; exit 2; }
          ${nativeEnv}
          export CARGO_TARGET_DIR="$here/apps/un/target"
          cd "$here"
          cargo build --release --manifest-path apps/un/Cargo.toml "$@"
          [ -x "$CARGO_TARGET_DIR/release/un" ] || { echo "the build produced no $CARGO_TARGET_DIR/release/un" >&2; exit 1; }
          echo "built: $CARGO_TARGET_DIR/release/un and .../un-cli"
        '';
      };

      models = pkgs.writeShellApplication {
        name = "un-models";
        runtimeInputs = with pkgs; [ curl python3 cacert ];
        text = ''
          here="''${1:-$PWD}"
          [ "$#" -gt 0 ] && shift
          [ -f "$here/tools/fetch_models.py" ] || { echo "no tools/fetch_models.py in $here: run this from the unengine checkout" >&2; exit 2; }
          export SSL_CERT_FILE="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
          mkdir -p "$here/models"
          for file in toys.onnx toys.json; do
            [ -s "$here/models/$file" ] && continue
            curl -fL --retry 3 -o "$here/models/$file" \
              "https://github.com/sealsbroken/unengine/releases/download/models-v1/$file"
          done
          python3 "$here/tools/fetch_models.py" --dest "$here/models" "$@"
        '';
      };

      run = pkgs.writeShellApplication {
        name = "un-run";
        text = ''
          here="''${1:-$PWD}"
          [ "$#" -gt 0 ] && shift
          app="$here/apps/un/target/release/un"
          [ -x "$app" ] || { echo "nothing built in $here: nix run .#build first" >&2; exit 2; }
          ${nativeEnv}
          # Models: UN_MODELS if set, else models/ next to the working directory.
          export UN_MODELS="''${UN_MODELS:-$here/models}"
          cd "$here"
          exec "$app" "$@"
        '';
      };

      # The release workflow's checks, minus the ones that need a camera, a microphone or a screen.
      smoke = pkgs.writeShellApplication {
        name = "un-smoke";
        runtimeInputs = with pkgs; [ imagemagick coreutils ];
        text = ''
          here="''${1:-$PWD}"
          bin="$here/apps/un/target/release"
          models="''${UN_MODELS:-$here/models}"
          [ -x "$bin/un-cli" ] || { echo "nothing built in $here: nix run .#build first" >&2; exit 2; }
          [ -f "$models/toys.onnx" ] || { echo "no toys.onnx in $models: nix run .#models" >&2; exit 2; }
          ${nativeEnv}
          cd "$bin"
          echo "== strategies =="
          out=$(./un-cli --models "$models" strategies)
          printf '%s\n' "$out"
          grep -q '^unmath-count-to-three' <<<"$out" || { echo "the unmath lessons are not registered" >&2; exit 1; }
          echo "== a scripted lesson on a drawn table, no camera and no microphone =="
          lesson=$(./un-cli --models "$models" lesson --sim 1 --strategy unmath-count-to-three --language ru \
            --voice console --mic off --answers "лес,один" --frames 120 2>&1)
          grep -m4 -E '^(labels|\[tutor\])' <<<"$lesson" || true
          grep -q '^\[tutor\] ' <<<"$lesson" || { echo "the tutor said nothing" >&2; exit 1; }
          echo "== the tutor's voice into the recogniser, RU then EN =="
          ./un-cli --models "$models" check-speech --language ru
          ./un-cli --models "$models" check-speech --language en
          echo "== the toy detector on a picture =="
          convert -size 640x480 xc:BurlyWood -fill Gold -draw "circle 310,240 370,240" table.jpg
          detected=$(./un-cli --models "$models" run --source table.jpg --frames 3 2>&1)
          grep -m1 '"raw"' <<<"$detected"
          rm -f table.jpg
          echo "== the window =="
          if [ -n "''${WAYLAND_DISPLAY:-}''${DISPLAY:-}" ]; then
            timeout 10 ./un >/dev/null 2>&1 && echo "the window exited at once" || echo "the window kept running (timeout killed it)"
          else
            echo "no DISPLAY or WAYLAND_DISPLAY here: skipped; on a desktop: nix run .#run"
          fi
          echo "smoke test: OK"
        '';
      };
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
            curl                       # nix run .#models: the models-v1 release
            cacert
            imagemagick                # the smoke test's drawn table
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
            echo "    the app:       nix run .#build, then nix run .#run"
            echo "    models:        nix run .#models    smoke test: nix run .#smoke"
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

      packages.${system} = {
        un-build = build;
        un-models = models;
        un-run = run;
        un-smoke = smoke;
      };

      apps.${system} = {
        build = { type = "app"; program = "${build}/bin/un-build"; };
        models = { type = "app"; program = "${models}/bin/un-models"; };
        run = { type = "app"; program = "${run}/bin/un-run"; };
        smoke = { type = "app"; program = "${smoke}/bin/un-smoke"; };
        default = { type = "app"; program = "${build}/bin/un-build"; };
      };
    };
}

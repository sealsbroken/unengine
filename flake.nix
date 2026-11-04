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
    in {
      devShells.${system}.default = pkgs.mkShell {
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
        '';
      };
    };
}

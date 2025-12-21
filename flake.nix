{
  description = "Deterministic Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs @ { flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];

      perSystem = { pkgs, system, ... }: let
        # Stable toolchain for regular tests
        stableToolchain = with inputs.fenix.packages.${system}; combine [
          stable.toolchain
        ];
        
        # Nightly toolchain for Miri
        nightlyToolchain = inputs.fenix.packages.${system}.latest.withComponents [
          "cargo"
          "rustc"
          "rustfmt"
          "miri"
        ];
        
        # Common build inputs
        commonBuildInputs = [
          pkgs.pkg-config
          pkgs.mold
          pkgs.bashInteractive
          pkgs.openssl
          pkgs.gcc
          pkgs.binutils
        ];
        
        # Helper function to create shell scripts with error handling
        # Sets up common build environment (gcc, binutils, mold, pkg-config, openssl)
        # Note: PKG_CONFIG_PATH should be sufficient for openssl-sys to find OpenSSL
        writeShellScriptWithError = name: script:
          toString (pkgs.writeShellScript name ''
            set -e
            export PATH="${pkgs.gcc}/bin:${pkgs.binutils}/bin:${pkgs.mold}/bin:${pkgs.pkg-config}/bin"
            export LD_LIBRARY_PATH="${pkgs.openssl.out}/lib"
            export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig"
            ${script}
          '');
      in {
        devShells.default = pkgs.mkShell {
          buildInputs = [
            stableToolchain
          ] ++ commonBuildInputs ++ [
            pkgs.mdbook
          ];

          LD_LIBRARY_PATH = "${pkgs.openssl.out}/lib";

          shellHook = ''
            echo "===================================="
            echo " Welcome to the deterministic dev shell! "
            echo "===================================="
            echo "Rust toolchain:"
            rustc --version
            echo "Cargo version:"
            cargo --version
            echo "LD_LIBRARY_PATH: $LD_LIBRARY_PATH"
            echo "===================================="
            echo "Ready to develop! 🦀"
          '';
        };

        # Miri devShell with nightly
        devShells.miri = pkgs.mkShell {
          buildInputs = [
            nightlyToolchain
          ] ++ commonBuildInputs;

          LD_LIBRARY_PATH = "${pkgs.openssl.out}/lib";

          shellHook = ''
            echo "Miri dev shell (nightly)"
            rustc --version
          '';
        };

        # Apps for CI/local execution
        apps.test = {
          type = "app";
          program = writeShellScriptWithError "test" ''
            export PATH="${stableToolchain}/bin:$PATH"
            cargo test
          '';
        };

        apps.test-all-features = {
          type = "app";
          program = writeShellScriptWithError "test-all-features" ''
            export PATH="${stableToolchain}/bin:$PATH"
            cargo test --all-features
          '';
        };

        apps.miri-test = {
          type = "app";
          program = writeShellScriptWithError "miri-test" ''
            export PATH="${nightlyToolchain}/bin:$PATH"
            # Ensure cargo uses the nightly toolchain from PATH (not rustup)
            unset RUSTUP_TOOLCHAIN
            # nix run preserves the working directory, but ensure we're in project root
            if [ ! -f "Cargo.toml" ]; then
              # Try to find project root
              while [ "$PWD" != "/" ] && [ ! -f "Cargo.toml" ]; do
                cd ..
              done
              if [ ! -f "Cargo.toml" ]; then
                echo "Error: Could not find Cargo.toml" >&2
                exit 1
              fi
            fi
            # Verify we're using nightly (using case-insensitive check without grep)
            RUSTC_VERSION=$(rustc --version)
            case "$RUSTC_VERSION" in
              *nightly*) ;;
              *) echo "Error: Not using nightly toolchain, got: $RUSTC_VERSION" >&2; exit 1;;
            esac
            # Verify cargo is also nightly
            CARGO_VERSION=$(cargo --version)
            case "$CARGO_VERSION" in
              *nightly*) ;;
              *) echo "Error: cargo is not nightly, got: $CARGO_VERSION" >&2; exit 1;;
            esac
            # Verify cargo-miri is available and comes from nightly
            if ! command -v cargo-miri >/dev/null 2>&1; then
              echo "Error: cargo-miri not found in PATH" >&2
              exit 1
            fi
            # Verify cargo-miri is from the nightly toolchain we set in PATH
            CARGO_MIRI_PATH=$(command -v cargo-miri)
            case "$CARGO_MIRI_PATH" in
              *nightly*) ;;
              *) echo "Error: cargo-miri not from nightly toolchain, found at: $CARGO_MIRI_PATH" >&2; exit 1;;
            esac
            cargo miri setup
            cargo miri test miri
          '';
        };

        apps.build-book = {
          type = "app";
          program = writeShellScriptWithError "build-book" ''
            export PATH="${pkgs.mdbook}/bin"
            cd book
            mdbook build
          '';
        };
      };
    };
}

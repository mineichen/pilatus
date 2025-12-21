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
        ];
        
        # Helper function to create shell scripts with error handling
        writeShellScriptWithError = name: script:
          toString (pkgs.writeShellScript name ''
            set -e
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
            cargo test
          '';
        };

        apps.test-all-features = {
          type = "app";
          program = writeShellScriptWithError "test-all-features" ''
            cargo test --all-features
          '';
        };

        apps.miri-test = {
          type = "app";
          program = writeShellScriptWithError "miri-test" ''
            export PATH="${nightlyToolchain}/bin:$PATH"
            export LD_LIBRARY_PATH="${pkgs.openssl.out}/lib"
            cargo miri setup
            cargo miri test miri
          '';
        };

        apps.build-book = {
          type = "app";
          program = writeShellScriptWithError "build-book" ''
            export PATH="${pkgs.mdbook}/bin:$PATH"
            cd book
            mdbook build
          '';
        };
      };
    };
}

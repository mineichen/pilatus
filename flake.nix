{
  description = "Deterministic Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    fenix.url = "github:nix-community/fenix";
  };

  outputs = { self, nixpkgs, flake-utils, fenix, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        rust = with fenix.packages.${system}; combine [
          stable.toolchain
          targets.wasm32-unknown-unknown.stable.rust-std
        ];
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = [
            rust
            pkgs.pkg-config
            pkgs.cmake
            pkgs.clang
            pkgs.stdenv.cc.cc.lib
            pkgs.bashInteractive
          ];

          LIBCLANG_PATH = "${pkgs.clang.cc.lib}/lib";
          PKG_CONFIG_PATH = "${pkgs.opencv}/lib/pkgconfig";
          LD_LIBRARY_PATH = "${pkgs.clang.cc.lib}/lib:${pkgs.stdenv.cc.cc.lib}/lib";

          shellHook = ''
            echo "===================================="
            echo " Welcome to the deterministic dev shell! "
            echo "===================================="
            echo "Rust toolchain:"
            rustc --version
            echo "Cargo version:"
            cargo --version
            echo "LIBCLANG_PATH: $LIBCLANG_PATH"
            echo "LD_LIBRARY_PATH: $LD_LIBRARY_PATH"
            echo "===================================="
            echo "Ready to develop! 🦀"
          '';
        };

        packages.miri-test = pkgs.writeShellScriptBin "miri-test" ''
          set -e
          echo "Running Miri tests..."
          cargo miri test simd_utils
        '';
      });
}

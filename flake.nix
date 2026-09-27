{
  description = "A flake for ocklog";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }@inputs:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

        buildToolchain = pkgs.rust-bin.nightly.latest.minimal.override {
          extensions = [ "rustc" "cargo" ];
        };

        rustPlatform = pkgs.makeRustPlatform {
          cargo = buildToolchain;
          rustc = buildToolchain;
        };

        commonBuildArgs = {
          src = pkgs.lib.cleanSource ./.;
          cargoLock = {
            lockFile = ./Cargo.lock;
          };
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ pkgs.openssl ];
          doCheck = false;
        };

      in {
        packages = {
          ocklog = rustPlatform.buildRustPackage (commonBuildArgs // {
            pname = "ocklog";
            version = "0.0.3";
          });

          default = pkgs.symlinkJoin {
            name = "ocklog-workspace";
            paths = [
              self.packages.${system}.ocklog
            ];
          };
        };
      });
}

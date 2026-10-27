{
  description = "Zellij sidebar plugin listing dev workspaces by name for session switching";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    rust-overlay,
    flake-utils,
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [(import rust-overlay)];
      };
      toolchain = pkgs.rust-bin.stable.latest.default.override {
        targets = ["wasm32-wasip1"];
      };
      rustPlatform = pkgs.makeRustPlatform {
        cargo = toolchain;
        rustc = toolchain;
      };
      wasm = rustPlatform.buildRustPackage {
        pname = "zj-redback-dev-status";
        version = "0.1.0";
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;

        buildPhase = ''
          runHook preBuild
          cargo build --release --offline --target wasm32-wasip1
          runHook postBuild
        '';

        installPhase = ''
          runHook preInstall
          mkdir -p $out
          cp target/wasm32-wasip1/release/zj-redback-dev-status.wasm \
            $out/zj-redback-dev-status.wasm
          runHook postInstall
        '';

        doCheck = false;
      };
    in {
      packages.default = wasm;
      packages.wasm = wasm;

      devShells.default = pkgs.mkShell {
        packages = [toolchain pkgs.zellij];
      };
    });
}

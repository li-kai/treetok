{
  description = "Development environment for treetok monorepo";

  nixConfig = {
    extra-substituters = [ "https://li-kai.cachix.org" ];
    extra-trusted-public-keys = [ "li-kai.cachix.org-1:hT/YtROuqsBhfSx1YDcMrFxBbnZLoyu+WA1CnhiUgWM=" ];
  };

  inputs = {
    fenix = {
      url = "github:nix-community/fenix/monthly";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      fenix,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        fenixPkgs = fenix.packages.${system};
        # Fenix Rust toolchain - use complete with targets
        rustToolchain =
          with fenixPkgs;
          combine [
            stable.rustc
            stable.cargo
            stable.rustfmt
            stable.clippy
            stable.rust-src
            targets.wasm32-unknown-unknown.stable.rust-std
          ];
        rustPlatform = pkgs.makeRustPlatform {
          cargo = fenixPkgs.stable.cargo;
          rustc = fenixPkgs.stable.rustc;
        };
      in
      {
        packages.default = rustPlatform.buildRustPackage {
          pname = "treetok";
          version = "0.1.4";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "--package" "treetok" ];
          nativeBuildInputs = with pkgs; [ pkg-config ];
        };

        devShells.default = pkgs.mkShell {
          name = "dev-environment";

          packages = [
            # Rust toolchain via fenix
            rustToolchain

            # Rust development tools
            pkgs.cargo-watch # Auto-rebuild on file changes
            pkgs.cargo-edit # cargo add/rm/upgrade commands
            pkgs.cargo-audit # Security vulnerability scanning
            pkgs.cargo-nextest # Faster test runner with better output
            pkgs.cargo-dist # Binary release packaging
            pkgs.rust-analyzer # Rust language server
            pkgs.just # Command runner

            # Development utilities
            pkgs.git
            pkgs.systemfd
          ];

          # Environment variables
          RUST_BACKTRACE = "1";

          # Shell hook
          shellHook = ''
            echo "Rust toolchain: $(rustc --version)"
          '';
        };
      }
    );
}

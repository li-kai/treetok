{
  description = "Development environment for treetok monorepo";

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
        # Fenix Rust toolchain - use complete with targets
        rustToolchain =
          with fenix.packages.${system};
          combine [
            stable.rustc
            stable.cargo
            stable.rustfmt
            stable.clippy
            stable.rust-src
            targets.wasm32-unknown-unknown.stable.rust-std
          ];
      in
      {
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

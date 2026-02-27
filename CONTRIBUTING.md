# Contributing to treetok

## Setup

### With Nix (recommended)

```bash
nix flake update    # Update dependencies
just build-nix      # Reproducible build
just run --help     # Verify binary
```

### With Cargo

Requires Rust 1.88+.

```bash
just build          # Build
just test           # Run tests
just check          # Lint
```

## Tasks

Run `just` to list all available recipes.

## Before submitting

1. `just check-all` — lints and tests
2. `just fmt` — format code
3. `just build-nix` — verify reproducible build
4. Update `DESIGN.md` if changing behavior

## Reproducibility

`nix build` produces a bit-for-bit identical binary (per OS/arch) for anyone with the same `flake.lock`. The lock pins:

- **nixpkgs** — system libraries and compiler binaries (exact git commit)
- **fenix** — Rust toolchain version (exact git commit)
- **Cargo.lock** — crate versions

To update dependencies: run `nix flake update`, then commit both `flake.nix` and `flake.lock`.

## Project structure

```
.
├── crates/
│   └── treetok/              # Main binary crate
│       ├── src/
│       │   ├── main.rs
│       │   ├── lib.rs
│       │   └── ...
│       ├── tests/            # Integration tests
│       └── Cargo.toml
├── Cargo.toml                # Workspace root
├── Cargo.lock                # Dependency lock
├── flake.nix                 # Nix build config
├── flake.lock                # Dependency pins (do not edit)
├── justfile                  # Task automation
├── DESIGN.md                 # Architecture and specification
└── README.md                 # User documentation
```

## Design

See [`DESIGN.md`](DESIGN.md) for architecture, CLI spec, tokenization strategy, error handling, and the feature roadmap.

**Priorities:** Accuracy > Performance > UX > Maintainability > Binary size

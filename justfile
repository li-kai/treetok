# Justfile for treetok workspace
# Install: cargo install just
# Usage: just build

set positional-arguments := true
set dotenv-load := true
set shell := ["bash", "-euo", "pipefail", "-c"]

# Default: show all recipes
default:
    @just --list

# Build all crates (use --release for release build)
build *args:
    cargo build --workspace {{ args }}

# Run tests (pass additional args, e.g., just test --no-capture)
# Doc tests run in parallel when no args are passed
[no-exit-message]
test *args:
    #!/usr/bin/env bash
    # -e intentionally omitted to capture individual exit codes
    set -uo pipefail
    if [[ $# -eq 0 ]]; then
        # Run doctests in background, capture output
        doctest_out=$(mktemp)
        trap 'rm -f "$doctest_out"' EXIT
        just doctest > "$doctest_out" 2>&1 &
        doctest_pid=$!

        # Run nextest in foreground
        nextest_ok=true
        cargo nextest run --workspace || nextest_ok=false

        # Wait for doctests and capture exit code
        doctest_ok=true
        wait $doctest_pid || doctest_ok=false

        # Show doctest results based on nextest outcome
        if $nextest_ok; then
            echo ""
            cat "$doctest_out"
        else
            # Just show summary
            if grep -q "^test result:" "$doctest_out"; then
                echo ""
                echo "Doc tests: $(grep "^test result:" "$doctest_out")"
            fi
        fi

        # Exit with failure if either failed
        $nextest_ok && $doctest_ok
    else
        cargo nextest run --workspace {{ args }}
    fi

# Run doc tests only (nextest doesn't support doc tests)
doctest *args:
    cargo test --workspace --doc -- --quiet {{ args }} 2>&1 | awk ' \
        /Doc-tests/ { header=$0; empty=1; next } \
        /running 0 tests/ { next } \
        /running [0-9]+ tests/ { print header; print; empty=0; next } \
        /test result:/ { if(!empty) print; next } \
        /^[[:space:]]*$/ { if(!empty) print; next } \
        !empty { print } \
    '

# Check code with clippy (no modifications)
check *args:
    cargo clippy --workspace --all-targets {{ args }} -- -D warnings

# Auto-fix clippy issues and format code
fix *args:
    cargo clippy --workspace --all-targets --fix --allow-dirty {{ args }} -- -D warnings
    just fmt

# Format code (use --check to verify without changing)
fmt *args:
    cargo fmt --all {{ args }}

# Watch and rebuild on changes (pass -x to override default build command)
watch *args='build':
    cargo watch -x {{ args }}

# Clean build artifacts
[confirm("This will delete all build artifacts. Continue?")]
clean:
    cargo clean

# Generate documentation (use --open to open in browser)
doc *args='--open':
    cargo doc --workspace --no-deps {{ args }}

# Run benchmarks
bench *args:
    cargo bench --workspace {{ args }}

# Run all checks (Rust)
check-all:
    just check
    just test

# Download latest ctoc vocab and re-embed it in the treetok binary
update-ctoc:
    cargo run -p xtask -- update-ctoc

# Tag and push a release (triggers cargo-dist CI)
release version:
    git tag "v{{ version }}"
    git push origin "v{{ version }}"

# Build binary reproducibly via Nix flake
build-nix *args:
    nix --extra-experimental-features nix-command --extra-experimental-features flakes build {{ args }}

# Run the built binary
run *args:
    ./result/bin/treetok {{ args }}


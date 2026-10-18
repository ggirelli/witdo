# List available recipes
default:
    @just --list

# Run the desktop app (extra arguments go to witdo, e.g. `just run --profile work`)
run *args:
    cargo run --locked -p witdo-desktop -- {{args}}

# Build the desktop app in release mode
build:
    cargo build --locked --release -p witdo-desktop

# Format all code
fmt:
    cargo fmt --all

# Check formatting without modifying files
fmt-check:
    cargo fmt --all -- --check

# Lint every crate and target, warnings are errors
clippy:
    cargo clippy --workspace --all-targets --locked -- -D warnings

# Run the test suite
test:
    cargo test --workspace --locked

# Check advisories, licenses, bans and sources
deny:
    cargo deny --locked check

# Everything CI checks
check: fmt-check clippy test deny

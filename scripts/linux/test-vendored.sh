#!/usr/bin/env sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-"$ROOT_DIR/target"}
export CARGO_TARGET_DIR

# Cargo does not run a non-workspace dependency's unit tests with `-p`.
# Test an isolated copy, seeded with the application's resolved versions.
# Offline resolution only prunes the unrelated lock entries; it cannot fetch
# newer dependencies or modify the source checkout's lockfiles.
SCRATCH=$(mktemp -d)
trap 'rm -rf "$SCRATCH"' EXIT HUP INT TERM
cp -a "$ROOT_DIR/vendor/smithay-clipboard/." "$SCRATCH/"
cp "$ROOT_DIR/Cargo.lock" "$SCRATCH/Cargo.lock"
# The upstream dev-dependency adds XKB solely for its unvendored interactive
# example. These library regressions use the same SCTK features as BExplorer.
sed '/^\[dev-dependencies.sctk\]/,$d' "$SCRATCH/Cargo.toml" > "$SCRATCH/Cargo.toml.test"
mv "$SCRATCH/Cargo.toml.test" "$SCRATCH/Cargo.toml"
cargo test --offline --manifest-path "$SCRATCH/Cargo.toml" --lib

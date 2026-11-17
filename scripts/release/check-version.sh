#!/bin/sh
# Fails unless the app config, the Rust crate and the npm package all name the same version.
#
#   scripts/release/check-version.sh [expected-version]
#
# Run it from the repository root. Pass the tag without the leading "v" to check a release tag too.
set -eu

tauri="$(sed -n 's/^  "version": "\([^"]*\)".*/\1/p' src-tauri/tauri.conf.json | head -n 1)"
crate="$(sed -n 's/^version = "\([^"]*\)".*/\1/p' src-tauri/Cargo.toml | head -n 1)"
npm="$(sed -n 's/^  "version": "\([^"]*\)".*/\1/p' package.json | head -n 1)"
expected="${1:-$tauri}"

echo "tauri.conf.json $tauri, Cargo.toml $crate, package.json $npm, expected $expected"
[ -n "$expected" ] && [ "$tauri" = "$expected" ] && [ "$crate" = "$expected" ] && [ "$npm" = "$expected" ] || {
  echo "version mismatch" >&2
  exit 1
}

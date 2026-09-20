#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$project_dir"
cargo build -p platform-macos --example adapter_builder --locked
# Match Cargo's configured output directory, including CARGO_TARGET_DIR.
target_dir=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
swiftc tools/adapter-builder/highlight.swift -o "$target_dir/debug/examples/adapter_highlight"
exec "$target_dir/debug/examples/adapter_builder" "$@"

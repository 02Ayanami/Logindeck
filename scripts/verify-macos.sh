#!/bin/sh
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repository_root"

if git grep -n 'platform_macos' -- app/src-tauri crates/native-host; then
  echo "desktop composition must use platform-runtime instead of platform-macos directly" >&2
  exit 1
fi

pnpm --dir app preflight

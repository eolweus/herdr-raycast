#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p bin
if command -v cargo >/dev/null; then
  cargo build --release
  cp target/release/herdr-raycast bin/herdr-raycast.tmp
  mv bin/herdr-raycast.tmp bin/herdr-raycast
elif [ -x bin/herdr-raycast ]; then
  echo "herdr-raycast: no cargo; keeping existing bin/herdr-raycast" >&2
else
  echo "herdr-raycast: cargo is required to build" >&2
  exit 1
fi

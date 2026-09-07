#!/usr/bin/env bash
# herdr runs plugin commands with a minimal PATH.
set -euo pipefail
export PATH="$HOME/dotfiles/bin:$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:${PATH:-}"
[ -n "${HERDR_BIN_PATH:-}" ] && PATH="$(dirname "$HERDR_BIN_PATH"):$PATH"

if [ -n "${HERDR_PLUGIN_ROOT:-}" ]; then
  ROOT="$HERDR_PLUGIN_ROOT"
else
  ROOT="$(cd "$(dirname "$0")/.." && pwd)"
fi
BIN="$ROOT/bin/herdr-raycast"
[ -x "$BIN" ] || BIN="$ROOT/target/release/herdr-raycast"
[ -x "$BIN" ] || BIN="$ROOT/target/debug/herdr-raycast"

exec "$BIN" "$@"

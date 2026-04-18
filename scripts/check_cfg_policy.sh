#!/usr/bin/env bash
#TODO: Add this script to a pre-commit git hook
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

had_error=0

print_violation() {
  local title="$1"
  echo "[cfg-policy] ${title}" >&2
}

if rg -n --no-heading '#\[cfg\(not\(' src >/dev/null; then
  print_violation "found disallowed #[cfg(not(...))] predicate(s) in src"
  rg -n --no-heading '#\[cfg\(not\(' src >&2 || true
  had_error=1
fi

if rg -n --no-heading '#\[cfg\(unix\)\]|#\[cfg\(windows\)\]' src >/dev/null; then
  print_violation "found disallowed legacy #[cfg(unix)]/#[cfg(windows)] predicate(s) in src"
  rg -n --no-heading '#\[cfg\(unix\)\]|#\[cfg\(windows\)\]' src >&2 || true
  had_error=1
fi

if rg -n --no-heading 'cfg!\(' src | rg -v '^src/platform.rs:' >/dev/null; then
  print_violation "found cfg!(...) outside src/platform.rs"
  rg -n --no-heading 'cfg!\(' src | rg -v '^src/platform.rs:' >&2 || true
  had_error=1
fi

if [[ "$had_error" -ne 0 ]]; then
  exit 1
fi

echo "[cfg-policy] OK"

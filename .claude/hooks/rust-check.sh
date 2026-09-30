#!/usr/bin/env bash
# PostToolUse hook (Write|Edit) — fast, scoped validation after a Rust file
# edit. Verified against the real repo commands in .claude/rules/rust.md /
# testing.md — does not invent commands.
#
# Deliberately does NOT run the full workspace test suite on every edit
# (`cargo test --workspace` takes minutes here — 23 crates). It runs the two
# genuinely fast checks (`cargo fmt --check`, `cargo check` scoped to the
# touched crate when derivable) and stops there. Broader verification
# (clippy -D warnings, cargo test -p <crate>, miri) is the session's job per
# .claude/rules/testing.md's change matrix, not this hook's.
#
# Hook contract (Claude Code): receives a JSON payload on stdin with at least
# tool_name / tool_input.file_path; exit 0 = pass (silent), exit 2 = blocking
# finding shown to the model, other non-zero = non-blocking error.
set -u

payload="$(cat)"

# Extract the edited file path. Prefer jq; fall back to a plain grep so this
# doesn't hard-fail on a minimal environment without jq installed.
if command -v jq >/dev/null 2>&1; then
  file_path="$(printf '%s' "$payload" | jq -r '.tool_input.file_path // empty' 2>/dev/null)"
else
  file_path="$(printf '%s' "$payload" | grep -o '"file_path"[[:space:]]*:[[:space:]]*"[^"]*"' | head -1 | sed -E 's/.*"file_path"[[:space:]]*:[[:space:]]*"([^"]*)".*/\1/')"
fi

# Not a Rust file (or path missing) — nothing to do.
case "$file_path" in
  *.rs) ;;
  *) exit 0 ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root" || exit 0

# Never run against the embedded firmware crate — it needs a sibling repo
# (../../INT) and a Cortex-M target not present in a normal dev checkout
# (see the exclusion comment in the workspace Cargo.toml).
case "$file_path" in
  */embedded/*) exit 0 ;;
esac

fmt_out="$(cargo fmt --all -- --check 2>&1)"
fmt_status=$?

# Scope `cargo check` to the touched crate when the path is under crates/<name>/,
# so this stays fast — a full `cargo check --workspace` is not a per-edit check.
crate=""
if [[ "$file_path" =~ crates/([^/]+)/ ]]; then
  crate="${BASH_REMATCH[1]}"
fi

if [[ -n "$crate" && "$crate" != "valori-embedded" && "$crate" != "valori-ffi" ]]; then
  check_out="$(cargo check -p "$crate" 2>&1)"
  check_status=$?
else
  check_out="$(cargo check --workspace --exclude valori-embedded --exclude valori-ffi 2>&1)"
  check_status=$?
fi

if [[ $fmt_status -ne 0 || $check_status -ne 0 ]]; then
  echo "rust-check hook: verification failed after editing $file_path" >&2
  if [[ $fmt_status -ne 0 ]]; then
    echo "--- cargo fmt --check ---" >&2
    echo "$fmt_out" >&2
  fi
  if [[ $check_status -ne 0 ]]; then
    echo "--- cargo check ---" >&2
    echo "$check_out" >&2
  fi
  exit 2
fi

exit 0

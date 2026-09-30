#!/usr/bin/env bash
# PreToolUse hook (Bash) — flags genuinely destructive command patterns
# before they run. Not a blanket blocker: it classifies, and only rejects
# the classes that are hard to undo or that destroy durable data specific
# to this repo (events.log, raft.redb, object-store snapshots — see
# .claude/skills/deploy-worker/SKILL.md).
#
# Hook contract (Claude Code): JSON payload on stdin with tool_input.command
# for a Bash tool call; exit 0 = allow (silent), exit 2 = blocking finding
# shown to the model (the model then needs explicit user approval to
# proceed some other way), other non-zero = non-blocking error.
#
# Deliberately avoids naive substring matching that would block safe uses
# (e.g. "grep -rn 'rm -rf' docs/" must not trigger this) by matching only at
# the start of the actual command (after stripping leading whitespace and
# a possible `sudo `), not anywhere in the string.
set -u

payload="$(cat)"

if command -v jq >/dev/null 2>&1; then
  cmd="$(printf '%s' "$payload" | jq -r '.tool_input.command // empty' 2>/dev/null)"
else
  cmd="$(printf '%s' "$payload" | grep -o '"command"[[:space:]]*:[[:space:]]*"[^"]*"' | head -1 | sed -E 's/.*"command"[[:space:]]*:[[:space:]]*"([^"]*)".*/\1/')"
fi

[[ -z "$cmd" ]] && exit 0

norm="$(printf '%s' "$cmd" | sed -E 's/^[[:space:]]*//; s/^sudo[[:space:]]+//')"

# Each pattern below is a documented, deliberate choice — not a generic
# deny-list. See the comment on each group for why it's here.
block=0
reason=""

# --- Filesystem: recursive/force delete outside anything scoped/obviously safe ---
if [[ "$norm" =~ ^rm[[:space:]].*(-[a-zA-Z]*r[a-zA-Z]*f|-[a-zA-Z]*f[a-zA-Z]*r) ]]; then
  block=1; reason="rm -rf (or -fr) — recursive force delete; this repo has no safe-to-assume scope for it"
fi

# --- Git: history-destroying / force-overwriting operations ---
if [[ "$norm" =~ ^git[[:space:]]+reset[[:space:]]+.*--hard ]]; then
  block=1; reason="git reset --hard — discards uncommitted work irreversibly"
elif [[ "$norm" =~ ^git[[:space:]]+clean[[:space:]]+.*-[a-zA-Z]*f[a-zA-Z]*d|^git[[:space:]]+clean[[:space:]]+.*-[a-zA-Z]*d[a-zA-Z]*f ]]; then
  block=1; reason="git clean -fd(x) — deletes untracked files/directories irreversibly"
elif [[ "$norm" =~ ^git[[:space:]]+push[[:space:]]+.*(--force|-f)([[:space:]]|$) ]]; then
  block=1; reason="git push --force/-f — can overwrite remote/shared history"
fi

# --- Database: this repo has no production SQL database, but a task could
# still be run against one belonging to the sibling valori-ui/backend repo
# from here by mistake — block the unambiguous destructive statements. ---
if [[ "$norm" =~ ^(DROP|TRUNCATE)[[:space:]] ]] || [[ "$norm" =~ [Dd][Rr][Oo][Pp][[:space:]]+(DATABASE|TABLE)[[:space:]] ]]; then
  block=1; reason="DROP/TRUNCATE — destructive SQL; this repo has no DB of its own, verify the target before proceeding"
fi

# --- Docker: this repo's durable data (events.log, raft.redb) can live in
# named volumes — see .claude/skills/deploy-worker/SKILL.md's "Never do". ---
if [[ "$norm" =~ ^docker[[:space:]]+compose[[:space:]]+down[[:space:]]+.*-v([[:space:]]|$) ]]; then
  block=1; reason="docker compose down -v — deletes volumes; may contain events.log/raft.redb"
elif [[ "$norm" =~ ^docker[[:space:]]+volume[[:space:]]+(rm|prune) ]]; then
  block=1; reason="docker volume rm/prune — can delete durable node state"
elif [[ "$norm" =~ ^docker[[:space:]]+system[[:space:]]+prune ]]; then
  block=1; reason="docker system prune — can delete volumes/images still needed for rollback"
fi

# --- Infra destroy ---
if [[ "$norm" =~ ^terraform[[:space:]]+destroy ]]; then
  block=1; reason="terraform destroy — destroys provisioned infrastructure (terraform/{aws,azure})"
elif [[ "$norm" =~ ^kubectl[[:space:]]+delete[[:space:]]+namespace ]]; then
  block=1; reason="kubectl delete namespace — destroys an entire namespace's resources"
elif [[ "$norm" =~ ^helm[[:space:]]+uninstall ]]; then
  block=1; reason="helm uninstall — removes the release; PVs may or may not survive depending on reclaim policy, verify first"
fi

if [[ $block -eq 1 ]]; then
  echo "dangerous-command-check hook: blocked — $reason" >&2
  echo "Command: $cmd" >&2
  echo "If this is genuinely intended, ask the user to confirm explicitly, then run it with their approval." >&2
  exit 2
fi

exit 0

#!/usr/bin/env bash
# hooks/check-drift.sh — Claude Code PreToolUse:Skill defense-in-depth.
#
# Fires whenever the agent is about to load a skill via the Skill tool. We
# filter to onchainos skills (okx-*), read the baked --expected-version from
# the Group A _shared/preflight.md (any one — they are byte-identical), then
# call `onchainos skills check`. On drift, emit a <system-reminder> line to
# stdout — Claude Code injects stdout into context before the skill loads.
#
# Always exits 0; we never block skill loading. Misses (no JSON, missing CLI,
# missing preflight) are silent — the regular preflight run is the primary
# enforcement.

set -euo pipefail

payload=$(cat)

# Extract skill_name from the tool_input JSON payload. Prefer python3 (always
# present on Claude-supported platforms); fall back to silent no-op if missing.
skill=$(printf '%s' "$payload" | python3 -c \
  'import json, sys
try:
    p = json.load(sys.stdin)
    print(p.get("tool_input", {}).get("skill_name", ""))
except Exception:
    pass' 2>/dev/null || echo "")

case "$skill" in
  okx-*) : ;;
  *)     exit 0 ;;
esac

command -v onchainos >/dev/null 2>&1 || exit 0

pf="${CLAUDE_PLUGIN_ROOT}/skills/okx-agentic-wallet/_shared/preflight.md"
[ -f "$pf" ] || exit 0

ver=$(grep -oE -- '--skill-version=[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9.-]+)?(\+[A-Za-z0-9.-]+)?' "$pf" \
      | head -1 | sed 's/.*=//')
[ -n "$ver" ] || exit 0

if ! out=$(onchainos skills check --expected-version="$ver" 2>&1); then
  printf '<system-reminder>onchainos skill drift detected: %s</system-reminder>\n' "$out"
fi
exit 0

#!/bin/sh

# Legacy compatibility wrapper for older prompts that still call this script.
# Current primary flow is documented in:
# skills/okx-agent-chat/ensure-okx-a2a-communication-ready.md
#
# Stdout is JSON only. `okx-a2a update` output is captured because update has
# no --json mode; the final readiness contract comes from
# `okx-a2a switch-runtime --json`.

FORMAT="json"
DETAIL_LIMIT=4000
A2A_NODE_PACKAGE="@okxweb3/a2a-node"

usage() {
  cat <<'EOF'
Usage: ensure-okx-a2a-ready.sh [--format json]

Legacy wrapper:
1. Bootstrap okx-a2a with npm if the CLI is missing.
2. Run `okx-a2a update` without --json and without --restart --yes.
3. Run `okx-a2a switch-runtime --json` and return that JSON.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --format)
      FORMAT="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'unknown argument: %s\n' "$1" >&2
      exit 2
      ;;
  esac
done

if [ "$FORMAT" != "json" ]; then
  printf 'only --format json is supported\n' >&2
  exit 2
fi

json_escape() {
  awk 'BEGIN { ORS = "" }
  {
    gsub(/\\/,"\\\\")
    gsub(/"/,"\\\"")
    gsub(/\t/,"\\t")
    gsub(/\r/,"\\r")
    if (NR > 1) {
      printf "\\n"
    }
    printf "%s", $0
  }'
}

jstr() {
  printf '"'
  printf '%s' "$1" | json_escape
  printf '"'
}

truncate_detail() {
  printf '%s' "$1" | awk -v limit="$DETAIL_LIMIT" '
    BEGIN { ORS = "" }
    {
      if (length(out) + length($0) + 1 <= limit) {
        if (out != "") out = out "\n"
        out = out $0
      }
    }
    END {
      print out
    }'
}

emit_result() {
  ok="$1"
  runtime="$2"
  state="$3"
  action="$4"
  reason="$5"
  user_message="$6"
  detail="$7"

  detail="$(truncate_detail "$detail")"

  printf '{\n'
  printf '  "ok": %s,\n' "$ok"
  printf '  "runtime": '; jstr "$runtime"; printf ',\n'
  printf '  "state": '; jstr "$state"; printf ',\n'
  printf '  "action": '; jstr "$action"; printf ',\n'
  printf '  "reason": '; jstr "$reason"; printf ',\n'
  printf '  "userMessage": '; jstr "$user_message"; printf ',\n'
  printf '  "detail": '; jstr "$detail"; printf '\n'
  printf '}\n'
}

run_capture() {
  CAPTURE_OUTPUT="$("$@" 2>&1)"
  CAPTURE_STATUS=$?
}

check_command() {
  command -v "$1" >/dev/null 2>&1
}

if ! check_command okx-a2a; then
  if ! check_command node; then
    emit_result false "unknown" "blocked" "none" "node_missing" \
      "Node.js is required to bootstrap OKX A2A communication." ""
    exit 0
  fi

  if ! check_command npm; then
    emit_result false "unknown" "blocked" "none" "npm_missing" \
      "npm is required to install $A2A_NODE_PACKAGE." ""
    exit 0
  fi

  run_capture npm install -g "$A2A_NODE_PACKAGE@latest"
  if [ "$CAPTURE_STATUS" -ne 0 ]; then
    emit_result false "unknown" "failed" "install_failed" "a2a_node_install_failed" \
      "Failed to install $A2A_NODE_PACKAGE." "$CAPTURE_OUTPUT"
    exit 0
  fi

  if ! check_command okx-a2a; then
    emit_result false "unknown" "blocked" "install_failed" "okx_a2a_not_on_path" \
      "okx-a2a was installed, but the global npm bin directory is not on PATH." "$CAPTURE_OUTPUT"
    exit 0
  fi
fi

run_capture okx-a2a update
if [ "$CAPTURE_STATUS" -ne 0 ]; then
  emit_result false "unknown" "failed" "update_failed" "okx_a2a_update_failed" \
    "Failed to update OKX A2A runtime integration." "$CAPTURE_OUTPUT"
  exit 0
fi

run_capture okx-a2a switch-runtime --json
if [ "$CAPTURE_STATUS" -ne 0 ]; then
  emit_result false "unknown" "failed" "switch_runtime_failed" "switch_runtime_failed" \
    "Failed to switch OKX A2A runtime." "$CAPTURE_OUTPUT"
  exit 0
fi

printf '%s\n' "$CAPTURE_OUTPUT"

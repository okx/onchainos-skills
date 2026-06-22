#!/bin/sh

# Deterministic OKX A2A runtime detection for the markdown router.
# Stdout is JSON only.

FORMAT="json"
RUNTIME_OVERRIDE=""

usage() {
  cat <<'EOF'
Usage: detect-okx-a2a-runtime.sh [--format json] [--runtime node|openclaw|hermes]

Detects the OKX A2A runtime branch after the native xmtp_refresh_agents check
has already failed in the current LLM toolset.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --format)
      FORMAT="${2:-}"
      shift 2
      ;;
    --runtime)
      RUNTIME_OVERRIDE="${2:-}"
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

jstr() {
  printf '"%s"' "$(printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g')"
}

emit_result() {
  ok="$1"
  runtime="$2"
  reason="$3"
  user_message="$4"

  printf '{\n'
  printf '  "ok": %s,\n' "$ok"
  printf '  "runtime": '; jstr "$runtime"; printf ',\n'
  printf '  "reason": '; jstr "$reason"; printf ',\n'
  printf '  "userMessage": '; jstr "$user_message"; printf '\n'
  printf '}\n'
}

detect_runtime() {
  if [ -n "$RUNTIME_OVERRIDE" ]; then
    case "$RUNTIME_OVERRIDE" in
      node|openclaw|hermes) printf '%s\n' "$RUNTIME_OVERRIDE"; return ;;
      *)
        printf 'invalid:%s\n' "$RUNTIME_OVERRIDE"
        return
        ;;
    esac
  fi

  if [ -n "${HERMES_SESSION_ID:-}" ] && [ -z "${HERMES_DESKTOP_CWD:-}" ]; then
    printf 'hermes\n'
    return
  fi

  if [ -n "${OPENCLAW_SHELL:-}" ] || [ -n "${OPENCLAW_CLI:-}" ]; then
    printf 'openclaw\n'
    return
  fi

  pid=$PPID
  for _ in 1 2 3 4 5 6 7 8; do
    if [ -z "$pid" ] || [ "$pid" = "0" ] || [ "$pid" = "1" ]; then
      break
    fi
    comm=$(ps -p "$pid" -o comm= 2>/dev/null | tr -d ' ')
    case "$comm" in
      *openclaw*|*OpenClaw*)
        printf 'openclaw\n'
        return
        ;;
    esac
    pid=$(ps -p "$pid" -o ppid= 2>/dev/null | tr -d ' ')
  done

  printf 'node\n'
}

runtime="$(detect_runtime)"
case "$runtime" in
  node|openclaw|hermes)
    emit_result true "$runtime" "" ""
    ;;
  invalid:*)
    invalid_value="${runtime#invalid:}"
    emit_result false "unknown" "invalid_runtime" "--runtime must be one of: node, openclaw, hermes. Got: $invalid_value"
    ;;
  *)
    emit_result false "$runtime" "unknown_runtime" "Could not determine the OKX A2A runtime."
    ;;
esac

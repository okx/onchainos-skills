#!/bin/sh

# Idempotent OKX A2A communication initialization for Node CLI runtimes.
# Stdout is JSON only; command output is captured and returned in `detail`.

FORMAT="json"
PROVIDER=""
RUNTIME="node"
DETAIL_LIMIT=4000
A2A_NODE_PACKAGE="@okxweb3/a2a-node"
# Advisory refresh threshold only. Readiness is decided by command capability
# checks below, not by a hard package-version gate.
A2A_NODE_UPDATE_BELOW_VERSION="0.0.5"

usage() {
  cat <<'EOF'
Usage: ensure-okx-a2a-ready.sh [--format json] [--provider codex|claude] [--runtime node]

Checks and initializes OKX A2A communication through the okx-a2a Node CLI.
The native xmtp_refresh_agents check and runtime branch routing must happen
before this script is called.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --format)
      FORMAT="${2:-}"
      shift 2
      ;;
    --provider)
      PROVIDER="${2:-}"
      shift 2
      ;;
    --runtime)
      RUNTIME="${2:-}"
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

json_array_csv() {
  csv="$1"
  printf '['
  first=true
  old_ifs=$IFS
  IFS=','
  # shellcheck disable=SC2086
  set -- $csv
  IFS=$old_ifs
  for item in "$@"; do
    [ -z "$item" ] && continue
    if [ "$first" = "true" ]; then
      first=false
    else
      printf ', '
    fi
    jstr "$item"
  done
  printf ']'
}

emit_result() {
  ok="$1"
  runtime="$2"
  state="$3"
  action="$4"
  reason="$5"
  user_message="$6"
  detail="$7"
  providers="${8:-}"
  current="${9:-}"
  required="${10:-}"
  next_command="${11:-}"

  detail="$(truncate_detail "$detail")"

  printf '{\n'
  printf '  "ok": %s,\n' "$ok"
  printf '  "runtime": '; jstr "$runtime"; printf ',\n'
  printf '  "state": '; jstr "$state"; printf ',\n'
  printf '  "action": '; jstr "$action"; printf ',\n'
  printf '  "reason": '; jstr "$reason"; printf ',\n'
  printf '  "userMessage": '; jstr "$user_message"; printf ',\n'
  printf '  "providers": '; json_array_csv "$providers"; printf ',\n'
  printf '  "current": '; jstr "$current"; printf ',\n'
  printf '  "required": '; jstr "$required"; printf ',\n'
  printf '  "nextCommand": '; jstr "$next_command"; printf ',\n'
  printf '  "detail": '; jstr "$detail"; printf '\n'
  printf '}\n'
}

run_capture() {
  CAPTURE_OUTPUT="$("$@" 2>&1)"
  CAPTURE_STATUS=$?
}

semver_ge() {
  awk -v cur="$1" -v req="$2" '
    function splitver(v, a) {
      gsub(/^[^0-9]*/, "", v)
      split(v, a, /[^0-9]+/)
    }
    BEGIN {
      splitver(cur, c)
      splitver(req, r)
      for (i = 1; i <= 3; i++) {
        ci = c[i] + 0
        ri = r[i] + 0
        if (ci > ri) exit 0
        if (ci < ri) exit 1
      }
      exit 0
    }'
}

check_command() {
  command -v "$1" >/dev/null 2>&1
}

node_version() {
  node --version 2>/dev/null
}

okx_a2a_version() {
  command_path="$(command -v okx-a2a 2>/dev/null)" || return 1
  node - "$command_path" <<'EOF' 2>/dev/null
const fs = require("fs");
const path = require("path");

try {
  let dir = path.dirname(fs.realpathSync(process.argv[2]));
  for (let i = 0; i < 8; i += 1) {
    const pkgPath = path.join(dir, "package.json");
    if (fs.existsSync(pkgPath)) {
      const pkg = JSON.parse(fs.readFileSync(pkgPath, "utf8"));
      if (pkg.name === "@okxweb3/a2a-node" && pkg.version) {
        process.stdout.write(pkg.version);
        process.exit(0);
      }
    }
    const next = path.dirname(dir);
    if (next === dir) break;
    dir = next;
  }
} catch (_) {
  // Fall through to failure below.
}

process.exit(1);
EOF
}

check_node_min() {
  min="$1"
  runtime="$2"
  if ! check_command node; then
    emit_result false "$runtime" "blocked" "none" "node_missing" \
      "Node.js >=$min is required for OKX A2A communication initialization." "" "" "" ">=$min" ""
    exit 0
  fi
  current="$(node_version)"
  if ! semver_ge "$current" "$min"; then
    emit_result false "$runtime" "blocked" "none" "node_version_too_old" \
      "Node.js >=$min is required for OKX A2A communication initialization." "" "" "$current" ">=$min" ""
    exit 0
  fi
}

detect_current_ai_provider() {
  codex_signal=false
  claude_signal=false

  if [ -n "${CODEX_THREAD_ID:-}" ] || [ "${CODEX_SHELL:-}" = "1" ] || [ "${CODEX_CI:-}" = "1" ]; then
    codex_signal=true
  fi

  if [ "${CLAUDECODE:-}" = "1" ]; then
    claude_signal=true
  fi

  if [ "$codex_signal" = "true" ] && [ "$claude_signal" != "true" ]; then
    printf 'codex\n'
  elif [ "$claude_signal" = "true" ] && [ "$codex_signal" != "true" ]; then
    printf 'claude\n'
  fi
}

provider_available() {
  status="$1"
  provider="$2"
  printf '%s' "$status" | grep -Eiq "\"?$provider\"?[[:space:]:=]+(true|yes)|\"?$provider\"?.{0,80}(installed|available)[^[:alnum:]]+(true|yes)"
}

available_providers() {
  status="$1"
  out=""
  for p in codex claude; do
    if provider_available "$status" "$p"; then
      if [ -z "$out" ]; then
        out="$p"
      else
        out="$out,$p"
      fi
    fi
  done
  printf '%s\n' "$out"
}

default_provider_from_status() {
  status="$1"
  printf '%s' "$status" | sed -n 's/.*default[":= ][":]*\([A-Za-z][A-Za-z0-9_-]*\).*/\1/p' | head -1
}

install_a2a_node() {
  runtime="$1"

  if ! check_command npm; then
    emit_result false "$runtime" "blocked" "none" "npm_missing" \
      "npm is required to install $A2A_NODE_PACKAGE." "" "" "" "" ""
    exit 0
  fi

  run_capture npm install -g "$A2A_NODE_PACKAGE@latest"
  if [ "$CAPTURE_STATUS" -ne 0 ]; then
    emit_result false "$runtime" "failed" "install_failed" "a2a_node_install_failed" \
      "Failed to install $A2A_NODE_PACKAGE." "$CAPTURE_OUTPUT" "" "" "" ""
    exit 0
  fi

  if ! check_command okx-a2a; then
    emit_result false "$runtime" "blocked" "install_failed" "okx_a2a_not_on_path" \
      "okx-a2a was installed, but the global npm bin directory is not on PATH." "$CAPTURE_OUTPUT" "" "" "" ""
    exit 0
  fi
}

try_update_a2a_node() {
  runtime="$1"

  if ! check_command npm; then
    install_output="npm is not available; skipped optional update for $A2A_NODE_PACKAGE."
    return 1
  fi

  run_capture npm install -g "$A2A_NODE_PACKAGE@latest"
  install_output="$CAPTURE_OUTPUT"
  if [ "$CAPTURE_STATUS" -ne 0 ]; then
    return 1
  fi

  if ! check_command okx-a2a; then
    install_output="$install_output

okx-a2a was not found on PATH after optional update."
    return 1
  fi

  updated_now=true
  return 0
}

ensure_a2a_node_available() {
  runtime="$1"

  installed_now=false
  updated_now=false
  install_output=""
  if ! check_command okx-a2a; then
    install_a2a_node "$runtime"
    install_output="$CAPTURE_OUTPUT"
    installed_now=true
  else
    current="$(okx_a2a_version || true)"
    if [ -z "$current" ] || ! semver_ge "$current" "$A2A_NODE_UPDATE_BELOW_VERSION"; then
      try_update_a2a_node "$runtime" || true
    fi
  fi

  current="$(okx_a2a_version || true)"
  if ! check_command okx-a2a; then
    emit_result false "$runtime" "blocked" "install_failed" "okx_a2a_not_on_path" \
      "okx-a2a is not available on PATH after installing $A2A_NODE_PACKAGE." "$install_output" "" "$current" "" ""
    exit 0
  fi
}

configure_node_provider_if_needed() {
  runtime="$1"
  script_name="$2"

  run_capture okx-a2a ai-provider status
  if [ "$CAPTURE_STATUS" -ne 0 ]; then
    emit_result false "$runtime" "failed" "none" "ai_provider_status_failed" \
      "Could not read OKX A2A AI provider status." "$CAPTURE_OUTPUT" "" "" "" ""
    exit 0
  fi

  status_output="$CAPTURE_OUTPUT"
  providers="$(available_providers "$status_output")"
  current_provider="$(detect_current_ai_provider)"
  current_default="$(default_provider_from_status "$status_output")"

  if [ -n "$PROVIDER" ]; then
    case "$PROVIDER" in
      codex|claude) ;;
      *)
        emit_result false "$runtime" "blocked" "none" "invalid_provider" \
          "--provider must be one of the available provider names." "$status_output" "$providers" "$PROVIDER" "" ""
        exit 0
        ;;
    esac
    if ! provider_available "$status_output" "$PROVIDER"; then
      emit_result false "$runtime" "blocked" "none" "provider_unavailable" \
        "The selected AI provider is not available in okx-a2a." "$status_output" "$providers" "$PROVIDER" "" ""
      exit 0
    fi
    selected="$PROVIDER"
  elif [ -n "$current_provider" ] && provider_available "$status_output" "$current_provider"; then
    selected="$current_provider"
  elif [ -z "$providers" ]; then
    emit_result false "$runtime" "blocked" "none" "no_ai_provider" \
      "No supported AI provider is available for OKX A2A. Install or open Codex/Claude and retry." "$status_output" "" "" "" ""
    exit 0
  else
    emit_result false "$runtime" "needs_user_choice" "none" "ambiguous_ai_provider" \
      "Choose which available AI provider should be used for OKX A2A task communication." \
      "$status_output" "$providers" "$current_provider" "" "sh $script_name --format json --runtime node --provider <provider>"
    exit 0
  fi

  NODE_PROVIDER_SELECTED="$selected"
  NODE_PROVIDER_CHANGED=false
  if [ -n "$current_default" ] && [ "$current_default" != "$selected" ]; then
    NODE_PROVIDER_CHANGED=true
  fi

  run_capture okx-a2a config provider --provider "$selected"
  if [ "$CAPTURE_STATUS" -ne 0 ]; then
    emit_result false "$runtime" "failed" "none" "provider_config_failed" \
      "Could not configure the OKX A2A AI provider." "$CAPTURE_OUTPUT" "$providers" "$selected" "" ""
    exit 0
  fi
}

handle_node() {
  runtime="node"
  check_node_min "22.0.0" "$runtime"

  ensure_a2a_node_available "$runtime"
  configure_node_provider_if_needed "$runtime" "$0"

  if [ "$installed_now" = "true" ] || [ "${updated_now:-false}" = "true" ]; then
    run_capture okx-a2a restart
    if [ "$CAPTURE_STATUS" -ne 0 ]; then
      emit_result false "$runtime" "failed" "restart_failed" "restart_failed" \
        "Could not start the OKX A2A daemon." "$CAPTURE_OUTPUT" "" "" "" ""
      exit 0
    fi
    restart_output="$CAPTURE_OUTPUT"
    if [ "$installed_now" = "true" ]; then
      node_action="installed_started"
    else
      node_action="updated_started"
    fi
    emit_result true "$runtime" "ready" "$node_action" "" \
      'OKX A2A communication is ready. Review permission behavior with `okx-a2a config permissions --json`; use `okx-a2a agent bypass off` if you want manual approval for actions.' \
      "npm install/update output:
$install_output

daemon restart output:
$restart_output" "" "" "" ""
    exit 0
  fi

  run_capture okx-a2a status
  if [ "$CAPTURE_STATUS" -ne 0 ]; then
    emit_result false "$runtime" "failed" "none" "status_failed" \
      "Could not read OKX A2A daemon status." "$CAPTURE_OUTPUT" "" "" "" ""
    exit 0
  fi

  status_output="$CAPTURE_OUTPUT"
  if printf '%s' "$status_output" | grep -Eiq '"?state"?[[:space:]:=]+"?stopped"?|(^|[^[:alpha:]])stopped([^[:alpha:]]|$)'; then
    run_capture okx-a2a restart
    if [ "$CAPTURE_STATUS" -ne 0 ]; then
      emit_result false "$runtime" "failed" "restart_failed" "restart_failed" \
        "Could not start the OKX A2A daemon." "$CAPTURE_OUTPUT" "" "" "" ""
      exit 0
    fi
    emit_result true "$runtime" "ready" "started" "" \
      "OKX A2A server has started." "$CAPTURE_OUTPUT" "" "" "" ""
    exit 0
  fi

  if printf '%s' "$status_output" | grep -Eiq '"?state"?[[:space:]:=]+"?running"?|(^|[^[:alpha:]])running([^[:alpha:]]|$)'; then
    if [ "${NODE_PROVIDER_CHANGED:-false}" = "true" ]; then
      run_capture okx-a2a restart
      if [ "$CAPTURE_STATUS" -ne 0 ]; then
        emit_result false "$runtime" "failed" "restart_failed" "restart_failed" \
          "Could not restart the OKX A2A daemon with the selected AI provider." "$CAPTURE_OUTPUT" "" "${NODE_PROVIDER_SELECTED:-}" "" ""
        exit 0
      fi
      emit_result true "$runtime" "ready" "restarted_provider" "" \
        "OKX A2A server restarted with the selected AI provider." "$CAPTURE_OUTPUT" "" "${NODE_PROVIDER_SELECTED:-}" "" ""
      exit 0
    fi

    run_capture okx-a2a agent refresh
    if [ "$CAPTURE_STATUS" -ne 0 ]; then
      emit_result false "$runtime" "failed" "refresh_failed" "agent_refresh_failed" \
        "Could not refresh OKX A2A agent communication identities." "$CAPTURE_OUTPUT" "" "" "" ""
      exit 0
    fi
    emit_result true "$runtime" "ready" "refreshed" "" \
      "OKX A2A communication is ready." "$CAPTURE_OUTPUT" "" "" "" ""
    exit 0
  fi

  emit_result false "$runtime" "failed" "none" "unknown_status" \
    "OKX A2A daemon status was neither running nor stopped." "$status_output" "" "" "" ""
}

if [ "$RUNTIME" != "node" ]; then
  emit_result false "unknown" "blocked" "none" "invalid_runtime" \
    "--runtime must be node for this Node-only readiness script." "" "" "$RUNTIME" "node" ""
  exit 0
fi

handle_node

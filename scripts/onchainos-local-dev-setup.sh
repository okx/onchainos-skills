#!/usr/bin/env bash
# Manage checkout-local development entry points.

set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "$script_dir/.." && pwd -P)"
project_dir="$repo_root/.codex"
bin_dir="$project_dir/bin"
skills_dir="$project_dir/skills"
runtime_dir="$project_dir/runtime/onchainos"
a2a_runtime_dir="$project_dir/runtime/a2a"
tmp_dir="$project_dir/runtime/tmp"
ai_workspace_dir="$a2a_runtime_dir/workspace"
target_dir="$project_dir/build/cargo-target"
onchainos_binary="$target_dir/debug/onchainos"

usage() {
  cat <<'EOF'
Usage: scripts/onchainos-local-dev-setup.sh <init|skills|cli> [onchainos arguments...]

Commands:
  init       First-time project initialization. Builds the local CLI, links
             skills, requires global okx-a2a, then restarts its project-local daemon.
  skills     Refresh project-local skill links only. No Rust build or daemon action.
  cli        Build the local CLI. Any following arguments run that local CLI.

Environment:
  OKX_A2A_GLOBAL_BIN  Path to the already globally installed okx-a2a executable.
EOF
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "error: required command not found: $1" >&2
    exit 1
  }
}

absolute_executable() {
  local candidate="$1"
  if [[ "$candidate" != /* ]]; then
    candidate="$(command -v "$candidate" 2>/dev/null || true)"
  fi
  [[ -n "$candidate" && -x "$candidate" ]] || {
    echo "error: okx-a2a must reference an installed executable" >&2
    exit 1
  }
  cd -- "$(dirname -- "$candidate")"
  printf '%s/%s\n' "$(pwd -P)" "$(basename -- "$candidate")"
}

prepare_project_dirs() {
  mkdir -p "$bin_dir" "$skills_dir" "$runtime_dir" "$a2a_runtime_dir" "$tmp_dir" "$target_dir"
  chmod 700 "$runtime_dir" "$a2a_runtime_dir" "$tmp_dir"
}

refresh_skills() {
  prepare_project_dirs
  for skill_target in "$skills_dir"/*; do
    [[ -L "$skill_target" ]] || continue
    skill_source="$(readlink "$skill_target")"
    [[ "$(dirname -- "$skill_source")" == "$repo_root/skills" ]] || continue
    [[ "$(basename -- "$skill_target")" == "$(basename -- "$skill_source")" ]] || continue
    [[ -f "$skill_source/SKILL.md" ]] && continue
    rm -- "$skill_target"
  done
  for skill_source in "$repo_root"/skills/*; do
    [[ -f "$skill_source/SKILL.md" ]] || continue
    skill_name="$(basename -- "$skill_source")"
    skill_target="$skills_dir/$skill_name"
    if [[ -e "$skill_target" || -L "$skill_target" ]]; then
      if [[ -L "$skill_target" && "$(readlink "$skill_target")" == "$skill_source" ]]; then
        continue
      fi
      echo "error: refusing to replace existing project skill entry: $skill_target" >&2
      exit 1
    fi
    ln -s "$skill_source" "$skill_target"
  done
}

write_onchainos_wrapper() {
  cat > "$bin_dir/onchainos" <<EOF
#!/usr/bin/env bash
set -euo pipefail
export ONCHAINOS_HOME='$runtime_dir'
export ONCHAINOS_CREDENTIAL_STORE=file
export ONCHAINOS_SKIP_CLIENT_VERSION_GATE="\${ONCHAINOS_SKIP_CLIENT_VERSION_GATE:-true}"
export TMPDIR='$tmp_dir'
export OKX_AGENT_TASK_HOME='$a2a_runtime_dir'
export OKX_A2A_AI_CWD='$repo_root'
export OKX_A2A_AI_CODEX_ADD_DIRS='$skills_dir'
export PATH='$bin_dir':"\$PATH"
if [[ "\${1:-}" == "preflight" ]]; then
  printf '%s\n' '{"ok":true,"data":{"status":"skipped","preflightSkipped":true,"skipReason":"project-local-wrapper","action":null}}'
  exit 0
fi
exec '$onchainos_binary' "\$@"
EOF
  chmod 700 "$bin_dir/onchainos"
}

build_cli() {
  [[ -f "$repo_root/cli/Cargo.toml" ]] || {
    echo "error: expected cli/Cargo.toml under $repo_root" >&2
    exit 1
  }
  prepare_project_dirs
  require_command cargo
  CARGO_TARGET_DIR="$target_dir" cargo build --manifest-path "$repo_root/cli/Cargo.toml"
  [[ -x "$onchainos_binary" ]] || {
    echo "error: project-local onchainos binary not found: $onchainos_binary" >&2
    exit 1
  }
  write_onchainos_wrapper
}

write_a2a_wrapper() {
  local a2a_bin="$1"
  cat > "$bin_dir/okx-a2a" <<EOF
#!/usr/bin/env bash
set -euo pipefail

refresh_ai_workspace_codex() {
  local workspace_codex_dir='$ai_workspace_dir/.codex'

  link_into_workspace() {
    local source_path="\$1"
    local target_path="\$2"

    if [[ -L "\$target_path" ]]; then
      [[ "\$(readlink "\$target_path")" == "\$source_path" ]] && return
      echo "error: refusing to replace unexpected A2A workspace link: \$target_path" >&2
      return 1
    fi
    if [[ -e "\$target_path" ]]; then
      echo "error: refusing to replace unexpected A2A workspace entry: \$target_path" >&2
      return 1
    fi
    ln -s "\$source_path" "\$target_path"
  }

  mkdir -p "\$workspace_codex_dir/bin"
  link_into_workspace '$bin_dir/onchainos' "\$workspace_codex_dir/bin/onchainos"
  link_into_workspace '$bin_dir/okx-a2a' "\$workspace_codex_dir/bin/okx-a2a"
  link_into_workspace '$skills_dir' "\$workspace_codex_dir/skills"
}

export ONCHAINOS_HOME='$runtime_dir'
export ONCHAINOS_CREDENTIAL_STORE=file
export ONCHAINOS_SKIP_CLIENT_VERSION_GATE="\${ONCHAINOS_SKIP_CLIENT_VERSION_GATE:-true}"
export TMPDIR='$tmp_dir'
export OKX_AGENT_TASK_HOME='$a2a_runtime_dir'
export OKX_A2A_AI_CWD='$repo_root'
export OKX_A2A_AI_CODEX_ADD_DIRS='$skills_dir'
export PATH='$bin_dir':"\$PATH"

if [[ "\${1:-}" == "daemon" && ( "\${2:-}" == "start" || "\${2:-}" == "restart" ) ]]; then
  '$a2a_bin' "\$@"
  refresh_ai_workspace_codex
  exit 0
fi

exec '$a2a_bin' "\$@"
EOF
  chmod 700 "$bin_dir/okx-a2a"
}

mode="${1:-help}"
case "$mode" in
  init)
    refresh_skills
    build_cli
    configured_a2a_bin="${OKX_A2A_GLOBAL_BIN:-$(command -v okx-a2a 2>/dev/null || true)}"
    a2a_bin="$(absolute_executable "$configured_a2a_bin")"
    if [[ "$a2a_bin" == "$bin_dir/okx-a2a" ]]; then
      echo "error: refusing to wrap this project's existing okx-a2a wrapper; set OKX_A2A_GLOBAL_BIN to the globally installed executable" >&2
      exit 1
    fi
    write_a2a_wrapper "$a2a_bin"
    "$bin_dir/okx-a2a" --version >/dev/null
    "$bin_dir/okx-a2a" daemon restart --provider codex
    printf 'Initialized local development runtime.\n'
    printf '  Skills:  %s\n' "$skills_dir"
    printf '  CLI:     %s\n' "$bin_dir/onchainos"
    printf '  A2A:     %s\n' "$bin_dir/okx-a2a"
    ;;
  skills)
    refresh_skills
    printf 'Refreshed project-local skills: %s\n' "$skills_dir"
    printf 'Start a new Codex session to load skill changes.\n'
    ;;
  cli)
    shift
    build_cli
    if (($#)); then
      exec "$bin_dir/onchainos" "$@"
    fi
    printf 'Built local CLI: %s\n' "$bin_dir/onchainos"
    ;;
  -h|--help|help)
    usage
    ;;
  *)
    echo "error: unknown development command: $mode" >&2
    usage >&2
    exit 2
    ;;
esac

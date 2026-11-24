#!/usr/bin/env bash
# Source this file in an interactive shell after `npm run setup`.
# Explicit arguments are preserved. Only this worktree's development profile
# receives its runtime; other profiles and workspaces keep their environment.

codex() {
  local codex_bin root profile arg selected_profile="" selected_workspace=""
  local next_is_cd=0 next_is_profile=0 next_is_feature=0
  local has_explicit_workspace_or_profile=0 has_shell_snapshot_choice=0 use_project_runtime=0

  if [[ -n "${ZSH_VERSION:-}" ]]; then
    codex_bin="$(whence -p codex)"
  else
    codex_bin="$(type -P codex)"
  fi
  [[ -n "$codex_bin" ]] || {
    printf '%s\n' 'error: codex executable was not found on PATH' >&2
    return 127
  }
  root="$(git -C "$PWD" rev-parse --show-toplevel 2>/dev/null)" || {
    command "$codex_bin" "$@"
    return
  }
  [[ -r "$root/.codex/profile-name" ]] || {
    command "$codex_bin" "$@"
    return
  }
  IFS= read -r profile < "$root/.codex/profile-name"
  [[ "$profile" =~ ^[A-Za-z0-9_-]+$ ]] || {
    command "$codex_bin" "$@"
    return
  }

  for arg in "$@"; do
    if (( next_is_cd )); then
      has_explicit_workspace_or_profile=1
      selected_workspace="$arg"
      next_is_cd=0
      continue
    fi
    if (( next_is_profile )); then
      has_explicit_workspace_or_profile=1
      selected_profile="$arg"
      next_is_profile=0
      continue
    fi
    if (( next_is_feature )); then
      [[ "$arg" == shell_snapshot ]] && has_shell_snapshot_choice=1
      next_is_feature=0
      continue
    fi
    case "$arg" in
      -C|--cd) next_is_cd=1 ;;
      -C?*) has_explicit_workspace_or_profile=1; selected_workspace="${arg#-C}" ;;
      --cd=*) has_explicit_workspace_or_profile=1; selected_workspace="${arg#*=}" ;;
      -p|--profile) next_is_profile=1 ;;
      -p?*) has_explicit_workspace_or_profile=1; selected_profile="${arg#-p}" ;;
      --profile=*) has_explicit_workspace_or_profile=1; selected_profile="${arg#*=}" ;;
      --disable|--enable) next_is_feature=1 ;;
      --disable=shell_snapshot|--enable=shell_snapshot) has_shell_snapshot_choice=1 ;;
    esac
  done

  if [[ "$selected_profile" == "$profile" ]]; then
    if [[ -z "$selected_workspace" ]]; then
      use_project_runtime=1
    elif [[ "$(cd -P -- "$selected_workspace" 2>/dev/null && pwd -P)" == "$root" ]]; then
      use_project_runtime=1
    fi
  fi

  if (( use_project_runtime )); then
    bash -c 'source "$1"; load_dev_runtime; shift; exec "$@"' \
      -- "$root/scripts/dev/common.sh" "$codex_bin" "$@"
  elif (( has_explicit_workspace_or_profile )); then
    command "$codex_bin" "$@"
  else
    if (( has_shell_snapshot_choice )); then
      bash -c 'source "$1"; load_dev_runtime; shift; exec "$@"' \
        -- "$root/scripts/dev/common.sh" "$codex_bin" -C "$root" -p "$profile" "$@"
    else
      bash -c 'source "$1"; load_dev_runtime; shift; exec "$@"' \
        -- "$root/scripts/dev/common.sh" "$codex_bin" -C "$root" -p "$profile" --disable shell_snapshot "$@"
    fi
  fi
}

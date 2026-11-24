#!/usr/bin/env bash

set -euo pipefail

dev_script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
dev_repo_root="$(cd -- "$dev_script_dir/../.." && pwd -P)"
load_dev_runtime() {
  export ONCHAINOS_HOME="$dev_repo_root/.codex/runtime/onchainos"
  export ONCHAINOS_CREDENTIAL_STORE="file"
  export ONCHAINOS_FORCE_FILE_KEYRING="1"
  export OKX_AGENT_TASK_HOME="$dev_repo_root/.codex/runtime/a2a"
  export ONCHAINOS_A2A_SPOOL_DIR="$dev_repo_root/.codex/runtime/a2a-spool"
  export TMPDIR="$dev_repo_root/.codex/runtime/tmp"
  export CARGO_HOME="$dev_repo_root/.codex/build/cargo-home"
  export CARGO_TARGET_DIR="$dev_repo_root/.codex/build/cargo-target"
  export PATH="$dev_repo_root/.codex/bin:$PATH"

  mkdir -p "$ONCHAINOS_HOME" "$OKX_AGENT_TASK_HOME" "$ONCHAINOS_A2A_SPOOL_DIR" "$TMPDIR" "$CARGO_HOME" "$CARGO_TARGET_DIR"
  chmod 700 "$ONCHAINOS_HOME" "$OKX_AGENT_TASK_HOME" "$ONCHAINOS_A2A_SPOOL_DIR" "$TMPDIR" "$CARGO_HOME"
  umask 077
}

#!/usr/bin/env bash

set -euo pipefail

dev_script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
dev_repo_root="$(cd -- "$dev_script_dir/../.." && pwd -P)"
dev_config="$dev_repo_root/.codex/dev.json"

load_dev_config() {
  [[ -f "$dev_config" ]] || {
    echo "error: missing $dev_config; run npm run dev:init" >&2
    exit 1
  }

  local selected
  selected="$(node -e '
    const fs = require("node:fs");
    const path = process.argv[1];
    let value;
    try { value = JSON.parse(fs.readFileSync(path, "utf8")); }
    catch (error) { console.error(`error: invalid ${path}: ${error.message}`); process.exit(1); }
    const keys = Object.keys(value);
    if (keys.length !== 1 || keys[0] !== "environment" || typeof value.environment !== "string" || !value.environment.trim()) {
      console.error(`error: ${path} must contain exactly one non-empty string field: environment`);
      process.exit(1);
    }
    process.stdout.write(value.environment.trim());
  ' "$dev_config")"

  case "$selected" in
    beta)
      export ONCHAINOS_ENV="beta"
      export OKX_BASE_URL="https://beta.okex.org"
      ;;
    production)
      export ONCHAINOS_ENV="production"
      export OKX_BASE_URL="https://web3.okx.com"
      ;;
    http://*|https://*)
      export ONCHAINOS_ENV="custom"
      export OKX_BASE_URL="$selected"
      ;;
    *)
      echo "error: unsupported environment '$selected' (use beta, production, or an http(s) URL)" >&2
      exit 2
      ;;
  esac

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
}

#!/usr/bin/env bash

set -euo pipefail

script_source="${BASH_SOURCE[0]}"
while [[ -L "$script_source" ]]; do
  script_source_dir="$(cd -P -- "$(dirname -- "$script_source")" && pwd)"
  script_source="$(readlink -- "$script_source")"
  [[ "$script_source" = /* ]] || script_source="$script_source_dir/$script_source"
done
script_dir="$(cd -P -- "$(dirname -- "$script_source")" && pwd)"
# shellcheck source=common.sh
source "$script_dir/common.sh"
load_dev_config

real_a2a="$dev_repo_root/.codex/bin/okx-a2a.real"
[[ -x "$real_a2a" ]] || {
  echo "error: global okx-a2a is unavailable; run npm run dev:init" >&2
  exit 1
}

local_onchainos="$CARGO_TARGET_DIR/debug/onchainos"
[[ -x "$local_onchainos" ]] || {
  echo "error: local OnchainOS CLI is unavailable; run npm run dev:cli" >&2
  exit 1
}

# A2A starts several OnchainOS probes concurrently and applies a bounded
# timeout to each one. Point its child processes at the already-built binary;
# routing them back through the development wrapper would start concurrent
# Cargo builds and can exhaust that timeout before the API request begins.
export ONCHAINOS_BIN="$local_onchainos"

local_codex="$dev_repo_root/.codex/bin/codex-a2a"
if [[ -x "$local_codex" ]]; then
  export OKX_A2A_AI_CODEX_COMMAND="$local_codex"
fi

exec "$real_a2a" "$@"

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
load_dev_runtime

real_a2a="$dev_repo_root/.codex/bin/okx-a2a.real"
[[ -x "$real_a2a" ]] || {
  echo "error: project-local okx-a2a is unavailable; run npm run setup" >&2
  exit 1
}

export OKX_AGENT_TASK_CLI_PATH="$dev_repo_root/.codex/a2a/node_modules/@okxweb3/a2a-node/dist/cli.js"
exec "$real_a2a" "$@"

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

exec "$real_a2a" "$@"

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

binary="$CARGO_TARGET_DIR/debug/onchainos"

[[ -x "$binary" ]] || {
  echo "error: local CLI was not built: $binary" >&2
  echo "hint: run npm run build after changing CLI source" >&2
  exit 1
}
exec "$binary" "$@"

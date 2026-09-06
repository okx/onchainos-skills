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

manifest="$dev_repo_root/cli/Cargo.toml"
binary="$CARGO_TARGET_DIR/debug/onchainos"

[[ -f "$manifest" ]] || { echo "error: missing $manifest" >&2; exit 1; }
command -v cargo >/dev/null 2>&1 || { echo "error: cargo is required" >&2; exit 1; }

# Keep the endpoint runtime-selectable. Passing it into Cargo would bake the
# selected URL into the binary and cause unnecessary rebuilds when switching.
env -u OKX_BASE_URL \
  CARGO_HOME="$CARGO_HOME" \
  CARGO_TARGET_DIR="$CARGO_TARGET_DIR" \
  cargo build --quiet --manifest-path "$manifest" --bin onchainos

[[ -x "$binary" ]] || { echo "error: local CLI was not built: $binary" >&2; exit 1; }
exec "$binary" "$@"

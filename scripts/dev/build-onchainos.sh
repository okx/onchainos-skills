#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd -P -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=common.sh
source "$script_dir/common.sh"
load_dev_runtime

manifest="$dev_repo_root/cli/Cargo.toml"
binary="$CARGO_TARGET_DIR/debug/onchainos"

[[ -f "$manifest" ]] || { echo "error: missing $manifest" >&2; exit 1; }
command -v cargo >/dev/null 2>&1 || { echo "error: cargo is required" >&2; exit 1; }

configured_base_url=""
env_file="$dev_repo_root/cli/.env"
if [[ -f "$env_file" ]]; then
  configured_base_url="$(node --input-type=module -e '
    import fs from "node:fs";
    const content = fs.readFileSync(process.argv[1], "utf8");
    let endpoint = "";
    for (const line of content.split(/\r?\n/)) {
      const trimmed = line.trim();
      if (!trimmed || trimmed.startsWith("#")) continue;
      const match = trimmed.match(/^OKX_BASE_URL\s*=\s*(.*)$/);
      if (match) endpoint = match[1].trim().replace(/^"|"$/g, "");
    }
    process.stdout.write(endpoint);
  ' "$env_file")"
fi
echo "Build OKX_BASE_URL: ${configured_base_url:-https://web3.okx.com (default)}"

env CARGO_HOME="$CARGO_HOME" \
  CARGO_TARGET_DIR="$CARGO_TARGET_DIR" \
  cargo build --quiet --manifest-path "$manifest" --bin onchainos

[[ -x "$binary" ]] || { echo "error: local CLI was not built: $binary" >&2; exit 1; }
echo "Built local OnchainOS CLI: $binary"

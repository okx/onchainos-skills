#!/usr/bin/env bash
set -euo pipefail

SKILL_DIR="$HOME/.claude/skills"
REF="develop"
BASE_API="https://api.github.com/repos/okx/onchainos-skills"

SKILLS=(
  okx-agentic-wallet
  okx-audit-log
  okx-defi-invest
  okx-defi-portfolio
  okx-dex-market
  okx-dex-signal
  okx-dex-swap
  okx-dex-token
  okx-dex-trenches
  okx-dex-ws
  okx-onchain-gateway
  okx-security
  okx-wallet-portfolio
  okx-x402-payment
)

echo "Installing OKX skills from develop branch → $SKILL_DIR"
echo ""

# Strip com.apple.provenance xattr from existing files (blocks overwrites on macOS)
echo "Clearing provenance xattrs..."
for skill in "${SKILLS[@]}"; do
  skill_path="$SKILL_DIR/$skill"
  if [[ -d "$skill_path" ]]; then
    find "$skill_path" -type f | while read -r f; do
      xattr -d com.apple.provenance "$f" 2>/dev/null || true
    done
  fi
done

# Download a file from GitHub API (base64-encoded) to a destination path
# Uses temp file to avoid partial writes
download_file() {
  local api_path="$1"
  local dest="$2"
  local tmp="${dest}.tmp.$$"
  mkdir -p "$(dirname "$dest")"
  curl -sSL "$BASE_API/contents/$api_path?ref=$REF" \
    | python3 -c "import sys,json,base64; d=json.load(sys.stdin); sys.stdout.buffer.write(base64.b64decode(d['content']))" \
    > "$tmp"
  mv "$tmp" "$dest"
  echo "    $api_path"
}

# Recursively sync a skill directory from GitHub API
# Writes item list to a temp file to avoid subshell issues with pipe+while
sync_dir() {
  local api_path="$1"
  local items_file
  items_file=$(mktemp)
  curl -sSL "$BASE_API/contents/$api_path?ref=$REF" \
    | python3 -c "
import sys, json
data = json.load(sys.stdin)
for x in data:
    print(x['type'] + '\t' + x['path'])
" > "$items_file"

  while IFS=$'\t' read -r ftype fpath; do
    local local_path="$SKILL_DIR/${fpath#skills/}"
    if [[ "$ftype" == "file" ]]; then
      download_file "$fpath" "$local_path"
    elif [[ "$ftype" == "dir" ]]; then
      sync_dir "$fpath"
    fi
  done < "$items_file"

  rm -f "$items_file"
}

for skill in "${SKILLS[@]}"; do
  echo "  $skill..."
  sync_dir "skills/$skill"
done

echo ""
echo "All done! Skills installed to $SKILL_DIR"

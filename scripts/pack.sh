#!/bin/bash
set -e
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ZIP="$ROOT/onchainos.zip"

echo "=== Building ==="
cd "$ROOT/cli" && OKX_BASE_URL=https://beta.okex.org cargo build --profile dev-release --features debug-log

echo "=== Packaging ==="
STAGE=$(mktemp -d)
trap "rm -rf $STAGE" EXIT

AGENT_SKILLS=(okx-agent-task okx-agent-identity okx-agentic-wallet okx-agent-task-test okx-agent-chat)
for skill in "${AGENT_SKILLS[@]}"; do
  if [ -d "$ROOT/skills/$skill" ]; then
    mkdir -p "$STAGE/skills/$skill"
    cp -rf "$ROOT/skills/$skill/"* "$STAGE/skills/$skill/"
  fi
done

cp "$ROOT/cli/target/dev-release/onchainos" "$STAGE/"

cat > "$STAGE/install.sh" << 'INSTALL'
#!/bin/bash
set -e
DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$DIR"
xattr -dr com.apple.quarantine "$DIR" 2>/dev/null || true
chmod +x "$DIR/onchainos" 2>/dev/null || true
mkdir -p ~/.local/bin
cp -f onchainos ~/.local/bin/onchainos
chmod +x ~/.local/bin/onchainos
if ! echo "$PATH" | grep -q "$HOME/.local/bin"; then
  RC="$HOME/.zshrc"; [ ! -f "$RC" ] && RC="$HOME/.bashrc"
  echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$RC"
  export PATH="$HOME/.local/bin:$PATH"
fi
for skill_dir in skills/*/; do
  skill_name=$(basename "$skill_dir")
  mkdir -p "$HOME/.agents/skills/$skill_name"
  cp -rf "$skill_dir"* "$HOME/.agents/skills/$skill_name/"
done
echo ""
echo "=== install done ==="
echo "onchainos: $(~/.local/bin/onchainos --version 2>&1)"
echo "skills:    $(ls ~/.agents/skills/)"
INSTALL

cd "$STAGE" && rm -f "$ZIP" && zip "$ZIP" -r . --symlinks
echo "=== $ZIP ($(du -h "$ZIP" | cut -f1)) ==="
open -R "$ZIP"
echo ""
echo "=== 测试机安装（打开 Terminal 粘贴） ==="
echo 'f=$(ls -t ~/Downloads/onchainos*.zip | head -1) && rm -rf /tmp/onchainos && unzip -o "$f" -d /tmp/onchainos && bash /tmp/onchainos/install.sh'

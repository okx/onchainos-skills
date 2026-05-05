#!/bin/bash
set -e
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ZIP="$ROOT/onchainos.zip"

echo "=== Building ==="
cd "$ROOT/cli" && OKX_BASE_URL=https://beta.okex.org cargo install --path . --force --features debug-log

echo "=== Packaging ==="
STAGE=$(mktemp -d)
trap "rm -rf $STAGE" EXIT
mkdir -p "$STAGE/skills/okx-agent-task"
cp "$HOME/.cargo/bin/onchainos" "$STAGE/"
for f in SKILL.md buyer.md provider.md evaluator.md; do
  cp "$ROOT/skills/okx-agent-task/$f" "$STAGE/skills/okx-agent-task/"
done

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
mkdir -p ~/.agents/skills/okx-agent-task
cp -f skills/okx-agent-task/*.md ~/.agents/skills/okx-agent-task/
echo ""
echo "=== install done ==="
echo "onchainos: $(~/.local/bin/onchainos --version 2>&1)"
echo "skills:    $(ls ~/.agents/skills/okx-agent-task/)"
INSTALL

cd "$STAGE" && rm -f "$ZIP" && zip "$ZIP" -r . --symlinks
echo "=== $ZIP ($(du -h "$ZIP" | cut -f1)) ==="
open -R "$ZIP"
echo ""
echo "=== 测试机安装（打开 Terminal 粘贴） ==="
echo 'f=$(ls -t ~/Downloads/onchainos*.zip | head -1) && rm -rf /tmp/onchainos && unzip -o "$f" -d /tmp/onchainos && bash /tmp/onchainos/install.sh'

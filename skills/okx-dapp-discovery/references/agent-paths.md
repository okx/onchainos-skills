# Agent-aware skills path resolution

Step 2 of `SKILL.md` needs to `Read` a freshly-installed plugin's `SKILL.md`. The actual filesystem location varies by host agent (Claude Code, Cursor, Codex, OpenCode, OpenClaw, Augment, …) and by `skills` CLI version. Hard-coding `$HOME/.claude/skills/...` is wrong on most setups — modern `skills` CLI uses `~/.agents/skills/` as the unified store and lets each agent discover from there.

Use the resolution chain below. Each tier is a fallback for the previous one.

---

## Tier 1 — Authoritative lookup (preferred)

`npx skills list -g --json` returns one record per installed skill with the absolute `path`. Query it by skill name:

### With `jq`

```bash
PLUGIN_NAME="polymarket-plugin"   # or aave-v3-plugin / hyperliquid-plugin / pancakeswap-v3-plugin / morpho-plugin

PLUGIN_PATH=$(npx --yes skills@latest list -g --json 2>/dev/null \
  | jq -r --arg n "$PLUGIN_NAME" '.[] | select(.name == $n) | .path')

[ -n "$PLUGIN_PATH" ] && echo "$PLUGIN_PATH/SKILL.md"
```

### Without `jq` (Python 3 fallback)

```bash
PLUGIN_PATH=$(npx --yes skills@latest list -g --json 2>/dev/null \
  | python3 -c 'import sys,json,os; n=os.environ["PLUGIN_NAME"]; d=json.load(sys.stdin); m=[x for x in d if x["name"]==n]; print(m[0]["path"] if m else "")')

[ -n "$PLUGIN_PATH" ] && echo "$PLUGIN_PATH/SKILL.md"
```

If `$PLUGIN_PATH` is empty, the plugin is not installed — drop to Tier 3.

The CLI's JSON record looks like this (from a real run):

```json
{
  "name": "okx-dapp-discovery",
  "path": "/Users/<user>/.agents/skills/okx-dapp-discovery",
  "scope": "global",
  "agents": ["Augment", "Claude Code", "OpenClaw"]
}
```

The `agents` array tells you which host agents will see this skill in their session — useful for diagnosing "I installed it but my agent still doesn't see it".

---

## Tier 2 — Common default paths (offline fallback)

When `npx` is unavailable (no network, sandbox without registry access, etc.), probe these paths in order. The first one whose `SKILL.md` exists is the answer.

| Order | Path pattern | Provenance |
|------|--------------|------------|
| 1 | `~/.agents/skills/<plugin>/SKILL.md` | Default install target of modern `skills` CLI; works for Claude Code, Cursor, Augment, OpenClaw, and any agent that discovers from the unified directory |
| 2 | `~/.claude/skills/<plugin>/SKILL.md` | Legacy Claude Code install target (pre-`skills`-CLI direct copy / older versions) |
| 3 | `~/.cursor/skills/<plugin>/SKILL.md` | Cursor-specific install when `npx skills add ... --agent cursor` was used |
| 4 | `~/.config/opencode/skills/<plugin>/SKILL.md` | Per `.opencode/INSTALL.md` symlink layout (project root → `~/.config/opencode/skills/`) |
| 5 | `~/.openclaw/onchainos-skills/skills/<plugin>/SKILL.md` | Per `.openclaw/INSTALL.md` clone layout (only relevant when the plugin was installed alongside `onchainos-skills`, not as a standalone package) |
| 6 | `~/.codex/skills/<plugin>/SKILL.md` | Codex CLI (when `--agent codex` was used) |

Probe loop (bash):

```bash
PLUGIN_NAME="polymarket-plugin"
CANDIDATES=(
  "$HOME/.agents/skills/$PLUGIN_NAME/SKILL.md"
  "$HOME/.claude/skills/$PLUGIN_NAME/SKILL.md"
  "$HOME/.cursor/skills/$PLUGIN_NAME/SKILL.md"
  "$HOME/.config/opencode/skills/$PLUGIN_NAME/SKILL.md"
  "$HOME/.openclaw/onchainos-skills/skills/$PLUGIN_NAME/SKILL.md"
  "$HOME/.codex/skills/$PLUGIN_NAME/SKILL.md"
)
for p in "${CANDIDATES[@]}"; do
  [ -f "$p" ] && { echo "$p"; break; }
done
```

---

## Tier 3 — Diagnostic (when Tiers 1 + 2 both fail)

1. **Did the install actually succeed?**
   ```bash
   npx --yes skills@latest list -g | grep -F "$PLUGIN_NAME"
   ```
   No output → install never landed. Re-run:
   ```bash
   npx --yes skills@latest add okx/plugin-store --skill "$PLUGIN_NAME" --yes --global
   ```

2. **Is the file in an unexpected location?** (last-resort filesystem search)
   ```bash
   find "$HOME" -maxdepth 6 -path "*/skills/$PLUGIN_NAME/SKILL.md" 2>/dev/null
   ```
   Cap depth at 6 to avoid scanning the entire home tree.

3. **Is the host agent picking it up?** Check the JSON record's `agents` array (Tier 1). If the current host (e.g., "Claude Code") is not listed, the install completed but didn't register for this agent — re-run with explicit agent flag:
   ```bash
   npx --yes skills@latest add okx/plugin-store --skill "$PLUGIN_NAME" --yes --global --agent claude-code
   ```

4. **Is `skills` CLI itself broken?** Bypass `npx` cache:
   ```bash
   npm cache clean --force
   npx --yes skills@latest --version
   ```

---

## When this file is consulted

- Step 2 of `SKILL.md` (Rules 1 and 2) — to read the installed plugin's quickstart.
- The bootstrap layer of `okx-dapp-discovery` — never by individual DApp plugins (those are already loaded by the time their SKILL.md runs).

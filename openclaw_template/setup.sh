#!/bin/sh
# onchainos - OpenClaw template build script
set -e

INSTALL_DIR="$HOME/.local/bin"
SKILLS_DIR="$HOME/.onchainos/skills"
ONCHAINOS_BIN="$INSTALL_DIR/onchainos"

# --- 1. Install CLI + workflows -----------------------------
echo "[onchainos] Installing CLI + workflows..."

# Force install.sh to do a fresh check on the first install: with no existing
# binary, a stale `last_check` would cause the upstream installer to skip.
[ ! -f "$ONCHAINOS_BIN" ] && rm -f "$HOME/.onchainos/last_check"

curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh

if [ ! -x "$ONCHAINOS_BIN" ]; then
  echo "[onchainos] ERROR: install.sh did not produce $ONCHAINOS_BIN"
  exit 1
fi

# --- 2. PATH probe ------------------------------------------
echo "[onchainos] --- PATH probe ---"
RUNTIME_PATH="$PATH"
echo "[onchainos] PATH: $RUNTIME_PATH"
echo "[onchainos] writable dirs on PATH:"
echo "$RUNTIME_PATH" | tr ':' '\n' | while read -r d; do
  if [ -n "$d" ] && [ -d "$d" ] && [ -w "$d" ]; then
    echo "[onchainos]   $d"
  fi
done
NPM_PREFIX=""
NPM_BIN=""
if command -v npm >/dev/null 2>&1; then
  # `|| NPM_PREFIX=""` keeps `set -e` happy if npm config exits non-zero.
  NPM_PREFIX="$(npm config get prefix 2>/dev/null)" || NPM_PREFIX=""
  [ -n "$NPM_PREFIX" ] && NPM_BIN="$NPM_PREFIX/bin"
  echo "[onchainos] npm prefix:   $NPM_PREFIX"
fi
echo "[onchainos] --- end probe ---"

# --- 3. Symlink onchainos onto runtime PATH -----------------
LINKED=""
link_into() {
  d="$1"
  [ -z "$d" ] && return 1
  mkdir -p "$d" 2>/dev/null || return 1
  [ -w "$d" ] || return 1
  ln -sf "$ONCHAINOS_BIN" "$d/onchainos" || return 1
  LINKED="$d"
  echo "[onchainos] Symlinked into $d/onchainos"
  return 0
}

# Split RUNTIME_PATH on ':' via IFS so directories containing spaces
# survive the loop intact (unquoted `$(... | tr ':' ' ')` would shred them).
OLD_IFS="$IFS"
IFS=':'
# shellcheck disable=SC2086
set -- $RUNTIME_PATH
IFS="$OLD_IFS"
for d in "$@"; do
  [ -z "$d" ] && continue
  if link_into "$d"; then break; fi
done

if [ -z "$LINKED" ]; then
  for d in "$NPM_BIN" /usr/local/bin /usr/bin "$HOME/.npm-global/bin" "$HOME/bin"; do
    if link_into "$d"; then break; fi
  done
fi

# --- 4. Verify bare command resolves ------------------------
if VER="$(onchainos --version 2>/dev/null)"; then
  echo "[onchainos] $VER is on PATH (via $LINKED)"
else
  echo "[onchainos] ERROR: 'onchainos' not resolvable as a bare command."
  echo "[onchainos] Linked dir:    ${LINKED:-<none>}"
  echo "[onchainos] Binary at:     $ONCHAINOS_BIN"
  echo "[onchainos] Runtime PATH:  $RUNTIME_PATH"
  exit 1
fi

# --- 5. Install skills --------------------------------------
echo "[onchainos] Installing skills..."
mkdir -p "$SKILLS_DIR"

INSTALLED_VIA=""
if command -v npx >/dev/null 2>&1 && npx -y skills --help >/dev/null 2>&1; then
  echo "[onchainos] Trying: npx skills add okx/onchainos-skills -y -g"
  if npx -y skills add okx/onchainos-skills -y -g; then
    INSTALLED_VIA="npx"
    if [ -z "$(ls -A "$SKILLS_DIR" 2>/dev/null)" ]; then
      echo "[onchainos] $SKILLS_DIR empty - searching known node-module roots for installed skills..."
      # Discover skill location by finding any okx-*/SKILL.md under known
      # node_modules roots; this works for the full okx-* skill set, not just
      # okx-dex-*. Take the parent of the parent to land on the skills root.
      FOUND_PARENT=""
      NPX_CACHE="$HOME/.npm/_npx"
      for root in "$NPM_PREFIX/lib/node_modules" "$NPX_CACHE" /usr/local/lib/node_modules; do
        [ -z "$root" ] && continue
        [ -d "$root" ] || continue
        F="$(find "$root" -maxdepth 6 -type f -name 'SKILL.md' -path '*/okx-*/SKILL.md' 2>/dev/null | head -1)"
        if [ -n "$F" ]; then FOUND_PARENT="$(dirname "$(dirname "$F")")"; break; fi
      done
      if [ -n "$FOUND_PARENT" ]; then
        echo "[onchainos] Found skills at $FOUND_PARENT - copying okx-* dirs to $SKILLS_DIR"
        # Copy only okx-* skill dirs to avoid sweeping in node_modules,
        # READMEs, fixtures, or other package siblings.
        cp_status=0
        cp -r "$FOUND_PARENT"/okx-* "$SKILLS_DIR/" 2>/dev/null || cp_status=$?
        [ "$cp_status" != "0" ] && echo "[onchainos] cp exited with status $cp_status (some files may have been skipped)"
      fi
    fi
  fi
fi

if [ -z "$INSTALLED_VIA" ] || [ -z "$(ls -A "$SKILLS_DIR" 2>/dev/null)" ]; then
  echo "[onchainos] Falling back to git clone for skills"
  REPO_DIR="$HOME/.onchainos/_repo"
  if [ -d "$REPO_DIR/.git" ]; then
    git -C "$REPO_DIR" pull --ff-only || true
  else
    git clone https://github.com/okx/onchainos-skills "$REPO_DIR" || true
  fi
  cp_status=0
  cp -r "$REPO_DIR/skills/"* "$SKILLS_DIR/" 2>/dev/null || cp_status=$?
  [ "$cp_status" != "0" ] && echo "[onchainos] cp (git fallback) exited with status $cp_status"
  INSTALLED_VIA="git"
fi

SKILL_COUNT="$(ls -1 "$SKILLS_DIR" 2>/dev/null | wc -l | tr -d ' ')"
echo "[onchainos] Installed $SKILL_COUNT skills to $SKILLS_DIR/ (via $INSTALLED_VIA)"

# Hard-fail if no skills installed: prior `|| true` swallows both
# npx and git failure, leaving an empty skills dir but still writing
# the bootstrap gate. Catch it here before that happens.
if [ "$SKILL_COUNT" = "0" ]; then
  echo "[onchainos] ERROR: no skills installed in $SKILLS_DIR/ (npx + git clone both yielded nothing)"
  exit 1
fi

# --- 6. Bootstrap status ------------------------------------
# Use local time for the date stamp; the same script writes and the bootstrap
# gate reads on the same machine, so timezone is consistent end-to-end.
mkdir -p "$HOME/.onchainos"
echo "$(date +%Y-%m-%d) OK" > "$HOME/.onchainos/bootstrap_status"
echo "[onchainos] Setup complete."

#!/bin/sh
# onchainos - OpenClaw template build script
set -e

INSTALL_DIR="$HOME/.local/bin"
SKILLS_DIR="$HOME/.onchainos/skills"
ONCHAINOS_BIN="$INSTALL_DIR/onchainos"

# --- 1. Install CLI + workflows -----------------------------
echo "[onchainos] Installing CLI + workflows..."

[ ! -f "$ONCHAINOS_BIN" ] && rm -f "$HOME/.onchainos/last_check"

curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh

if [ ! -x "$ONCHAINOS_BIN" ]; then
  echo "[onchainos] ERROR: install.sh did not produce $ONCHAINOS_BIN"
  exit 1
fi

# --- 2. PATH probe ------------------------------------------
echo "[onchainos] --- PATH probe ---"
echo "[onchainos] build PATH:   $PATH"
RUNTIME_PATH="$(sh -c 'echo $PATH')"
echo "[onchainos] runtime PATH: $RUNTIME_PATH"
echo "[onchainos] writable dirs on runtime PATH:"
echo "$RUNTIME_PATH" | tr ':' '\n' | while read -r d; do
  if [ -n "$d" ] && [ -d "$d" ] && [ -w "$d" ]; then
    echo "[onchainos]   $d"
  fi
  :
done || true
NPM_PREFIX=""
NPM_BIN=""
if command -v npm >/dev/null 2>&1; then
  NPM_PREFIX="$(npm config get prefix 2>/dev/null)"
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

for d in $(echo "$RUNTIME_PATH" | tr ':' ' '); do
  [ -z "$d" ] && continue
  if link_into "$d"; then break; fi
done

if [ -z "$LINKED" ]; then
  for d in "$NPM_BIN" /usr/local/bin /usr/bin "$HOME/.npm-global/bin" "$HOME/bin"; do
    if link_into "$d"; then break; fi
  done
fi

# --- 4. Verify bare command resolves ------------------------
if sh -c 'command -v onchainos >/dev/null 2>&1 && onchainos --version >/dev/null 2>&1'; then
  echo "[onchainos] $(sh -c 'onchainos --version') is on PATH (via $LINKED)"
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
      echo "[onchainos] $SKILLS_DIR empty - searching for installed skills..."
      FOUND_PARENT=""
      for root in "$HOME" "$NPM_PREFIX" /usr/local/lib/node_modules; do
        [ -z "$root" ] && continue
        F="$(find "$root" -maxdepth 6 -type d -name 'okx-dex-*' 2>/dev/null | head -1)"
        if [ -n "$F" ]; then FOUND_PARENT="$(dirname "$F")"; break; fi
      done
      if [ -n "$FOUND_PARENT" ]; then
        echo "[onchainos] Found skills at $FOUND_PARENT - copying to $SKILLS_DIR"
        cp -r "$FOUND_PARENT"/* "$SKILLS_DIR/" 2>/dev/null || true
      fi
    fi
  fi
fi

if [ -z "$INSTALLED_VIA" ] || [ -z "$(ls -A "$SKILLS_DIR" 2>/dev/null)" ]; then
  echo "[onchainos] Falling back to git clone for skills"
  REPO_DIR="$HOME/.openclaw/onchainos-skills"
  if [ -d "$REPO_DIR/.git" ]; then
    git -C "$REPO_DIR" pull --ff-only || true
  else
    git clone https://github.com/okx/onchainos-skills "$REPO_DIR" || true
  fi
  cp -r "$REPO_DIR/skills/"* "$SKILLS_DIR/" 2>/dev/null || true
  INSTALLED_VIA="git"
fi

SKILL_COUNT="$(ls -1 "$SKILLS_DIR" 2>/dev/null | wc -l | tr -d ' ')"
echo "[onchainos] Installed $SKILL_COUNT skills to $SKILLS_DIR/ (via $INSTALLED_VIA)"

# --- 6. Bootstrap status ------------------------------------
mkdir -p "$HOME/.onchainos"
echo "$(date +%Y-%m-%d) OK" > "$HOME/.onchainos/bootstrap_status"
echo "[onchainos] Setup complete."

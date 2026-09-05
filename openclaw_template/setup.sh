#!/bin/sh
# onchainos - OpenClaw template build script
set -e

INSTALL_DIR="$HOME/.local/bin"
ONCHAINOS_BIN="$INSTALL_DIR/onchainos"

# --- 1. Install OnchainOS -----------------------------------
echo "[onchainos] Installing CLI, skills, and A2A runtime..."

npx -y oc-onchainos install

if [ ! -x "$ONCHAINOS_BIN" ]; then
  echo "[onchainos] ERROR: oc-onchainos did not produce $ONCHAINOS_BIN"
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

# --- 5. Bootstrap status ------------------------------------
# Use local time for the date stamp; the same script writes and the bootstrap
# gate reads on the same machine, so timezone is consistent end-to-end.
mkdir -p "$HOME/.onchainos"
echo "$(date +%Y-%m-%d) OK" > "$HOME/.onchainos/bootstrap_status"
echo "[onchainos] Setup complete."

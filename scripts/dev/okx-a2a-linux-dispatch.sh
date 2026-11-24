#!/usr/bin/env bash
# onchainos-dev generated Linux A2A dispatcher

set -euo pipefail

repo_root="$(git -C "${PWD:-.}" rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -n "$repo_root" && -x "$repo_root/.codex/bin/okx-a2a" ]]; then
  exec "$repo_root/.codex/bin/okx-a2a" "$@"
fi

for dir in ${PATH//:/ }; do
  candidate="$dir/okx-a2a"
  [[ "$candidate" = "$0" ]] && continue
  if [[ -x "$candidate" ]]; then
    exec "$candidate" "$@"
  fi
done

printf '%s\n' 'error: no okx-a2a executable is installed; run npx -y @okxweb3/onchainos-installer install' >&2
exit 127

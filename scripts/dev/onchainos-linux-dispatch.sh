#!/usr/bin/env bash
# onchainos-dev generated Linux CLI dispatcher

set -euo pipefail

repo_root="$(git -C "${PWD:-.}" rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -n "$repo_root" && -x "$repo_root/.codex/bin/onchainos" ]]; then
  exec "$repo_root/.codex/bin/onchainos" "$@"
fi

release="${HOME}/.local/bin/onchainos.release"
if [[ -x "$release" ]]; then
  exec "$release" "$@"
fi

printf '%s\n' 'error: no OnchainOS CLI is installed; run the installer or npm run setup' >&2
exit 127

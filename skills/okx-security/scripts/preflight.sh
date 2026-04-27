#!/usr/bin/env bash
# scripts/preflight.sh — onchainos session preflight (POSIX).
#
# Ships inside each skill's scripts/ folder. Invoked from SKILL.md or
# support files via a relative path from the skill root:
# `bash scripts/preflight.sh --skill-version=...`.
#
# Pipeline:
#   1. Ensure the onchainos CLI is available.
#   2. Defer the skill-version drift check to `onchainos skills check`.

set -euo pipefail

SKILL_VERSION=""
for arg in "$@"; do
  case "$arg" in
    --skill-version=*) SKILL_VERSION="${arg#*=}" ;;
  esac
done
[ -n "$SKILL_VERSION" ] || { echo "warn: preflight.sh missing --skill-version arg" >&2; exit 2; }

if ! command -v onchainos >/dev/null 2>&1; then
  echo "warn: onchainos CLI is not installed. Install it from https://github.com/okx/onchainos-skills#installation" >&2
  exit 2
fi

onchainos skills check --expected-version="$SKILL_VERSION"

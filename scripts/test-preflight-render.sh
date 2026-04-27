#!/usr/bin/env bash
# scripts/test-preflight-render.sh
#
# Simulates the release.yml preflight-render sed locally and verifies:
#   (a) all 6 _shared/preflight.md copies remain byte-identical,
#   (b) all 11 scripts/preflight.sh copies remain byte-identical (Group A + B + C),
#   (c) all 11 scripts/preflight.ps1 copies remain byte-identical,
#   (d) exactly 1 POSIX + 1 PowerShell invocation per target file at the new version,
#   (e) the sed is idempotent across two consecutive simulated releases,
#   (f) Group D skills (no preflight) gain neither invocations nor scripts.
#
# Operates on a sandboxed copy of skills/ — never modifies the working tree.
# Run before pushing changes that touch release.yml or any preflight content.

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

SANDBOX="$(mktemp -d "${TMPDIR:-/tmp}/preflight-render.XXXXXX")"
cleanup() {
  rm -rf "$SANDBOX"
}
trap cleanup EXIT

cp -R skills "$SANDBOX/skills"

# macOS (BSD sed) needs `sed -i ''`; Linux GNU sed uses `sed -i`.
# The release workflow runs on ubuntu-latest (GNU). Detect for local testing.
if sed --version >/dev/null 2>&1; then
  SED_INPLACE=(-i)          # GNU
else
  SED_INPLACE=(-i "")       # BSD/macOS
fi

GROUP_A_SHARED=(
  okx-agentic-wallet/_shared/preflight.md
  okx-dex-market/_shared/preflight.md
  okx-dex-signal/_shared/preflight.md
  okx-dex-swap/_shared/preflight.md
  okx-dex-token/_shared/preflight.md
  okx-dex-trenches/_shared/preflight.md
)

GROUP_C_INLINE=(
  okx-onchain-gateway/SKILL.md
  okx-security/SKILL.md
  okx-wallet-portfolio/SKILL.md
)

PRELIGHT_BEARING_FILES=("${GROUP_A_SHARED[@]}" "${GROUP_C_INLINE[@]}")

SCRIPT_SKILLS=(
  okx-agentic-wallet okx-dex-market okx-dex-signal okx-dex-swap okx-dex-token
  okx-dex-trenches okx-dex-ws okx-x402-payment
  okx-onchain-gateway okx-security okx-wallet-portfolio
)

GROUP_D_SKILLS=(okx-audit-log okx-defi-invest okx-defi-portfolio)

render() {
  local version="$1"
  for f in "$SANDBOX"/skills/*/_shared/preflight.md "$SANDBOX"/skills/*/SKILL.md; do
    [ -f "$f" ] || continue
    sed -E "${SED_INPLACE[@]}" \
      "s/--skill-version=[0-9]+\\.[0-9]+\\.[0-9]+(-[A-Za-z0-9.-]+)?(\\+[A-Za-z0-9.-]+)?/--skill-version=${version}/g" \
      "$f"
    sed -E "${SED_INPLACE[@]}" \
      "s/-SkillVersion [0-9]+\\.[0-9]+\\.[0-9]+(-[A-Za-z0-9.-]+)?(\\+[A-Za-z0-9.-]+)?/-SkillVersion ${version}/g" \
      "$f"
  done
}

assert_preflight_invocations() {
  local expected="$1"
  for relpath in "${PRELIGHT_BEARING_FILES[@]}"; do
    local f="$SANDBOX/skills/$relpath"
    local posix_count
    posix_count=$(grep -c "preflight.sh\" --skill-version=${expected}" "$f" || true)
    if [ "$posix_count" -ne 1 ]; then
      echo "FAIL: $relpath has $posix_count POSIX invocations of --skill-version=${expected}, expected 1" >&2
      exit 1
    fi
    local powershell_count
    powershell_count=$(grep -c "preflight.ps1\" -SkillVersion ${expected}" "$f" || true)
    if [ "$powershell_count" -ne 1 ]; then
      echo "FAIL: $relpath has $powershell_count PowerShell invocations of -SkillVersion ${expected}, expected 1" >&2
      exit 1
    fi
  done
}

assert_shared_identical() {
  local files=()
  for relpath in "${GROUP_A_SHARED[@]}"; do
    files+=("$SANDBOX/skills/$relpath")
  done
  local unique
  unique=$(shasum "${files[@]}" | awk '{print $1}' | sort -u | wc -l | tr -d ' ')
  if [ "$unique" -ne 1 ]; then
    echo "FAIL: _shared/preflight.md copies drifted apart (unique hashes: $unique)" >&2
    exit 1
  fi
}

assert_scripts_identical() {
  local sh_unique ps_unique
  sh_unique=$(shasum "$SANDBOX"/skills/*/scripts/preflight.sh | awk '{print $1}' | sort -u | wc -l | tr -d ' ')
  if [ "$sh_unique" -ne 1 ]; then
    echo "FAIL: skills/*/scripts/preflight.sh copies drifted (unique hashes: $sh_unique)" >&2
    exit 1
  fi
  ps_unique=$(shasum "$SANDBOX"/skills/*/scripts/preflight.ps1 | awk '{print $1}' | sort -u | wc -l | tr -d ' ')
  if [ "$ps_unique" -ne 1 ]; then
    echo "FAIL: skills/*/scripts/preflight.ps1 copies drifted (unique hashes: $ps_unique)" >&2
    exit 1
  fi
}

assert_script_count() {
  local n
  n=$(ls "$SANDBOX"/skills/*/scripts/preflight.sh 2>/dev/null | wc -l | tr -d ' ')
  if [ "$n" -ne ${#SCRIPT_SKILLS[@]} ]; then
    echo "FAIL: expected ${#SCRIPT_SKILLS[@]} skills/*/scripts/preflight.sh files, found $n" >&2
    exit 1
  fi
  for skill in "${SCRIPT_SKILLS[@]}"; do
    [ -f "$SANDBOX/skills/$skill/scripts/preflight.sh" ] || {
      echo "FAIL: skills/$skill/scripts/preflight.sh missing" >&2
      exit 1
    }
    [ -f "$SANDBOX/skills/$skill/scripts/preflight.ps1" ] || {
      echo "FAIL: skills/$skill/scripts/preflight.ps1 missing" >&2
      exit 1
    }
  done
}

assert_group_d_clean() {
  for skill in "${GROUP_D_SKILLS[@]}"; do
    if grep -q "scripts/preflight\." "$SANDBOX/skills/$skill/SKILL.md" 2>/dev/null; then
      echo "FAIL: skills/$skill/SKILL.md unexpectedly contains a preflight invocation" >&2
      exit 1
    fi
    for ext in sh ps1; do
      if [ -f "$SANDBOX/skills/$skill/scripts/preflight.$ext" ]; then
        echo "FAIL: skills/$skill/scripts/preflight.$ext should not exist (Group D out of scope)" >&2
        exit 1
      fi
    done
  done
}

assert_script_count
assert_scripts_identical

# First simulated release.
render 9.9.9
assert_shared_identical
assert_preflight_invocations 9.9.9
assert_group_d_clean

# Second simulated release (idempotency).
render 10.0.0
assert_shared_identical
assert_preflight_invocations 10.0.0
assert_group_d_clean

echo "OK: preflight render smoke test passed."

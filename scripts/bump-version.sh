#!/usr/bin/env bash
# bump-version.sh — interactively bump the semver across the repo.
#
# Updates a single new version into ALL of:
#   • cli/Cargo.toml          ([package] version)
#   • cli/Cargo.lock          (the onchainos-cli package entry)
#   • skills/*/SKILL.md       (the frontmatter `version:` field of every skill)
#   • package.json            (the top-level `"version"` field)
#   • .claude-plugin/plugin.json / .cursor-plugin/plugin.json / .codex-plugin/plugin.json
#                             (the top-level `"version"` field)
#
#   MAJOR   incompatible API changes              (4.0.0)
#   MINOR   backwards-compatible new functionality (3.3.0)
#   PATCH   backwards-compatible bug fixes         (3.2.1)
#   CUSTOM  any explicit x.y.z you type in
#
# Usage: bash scripts/bump-version.sh                       (interactive prompt)
#        bash scripts/bump-version.sh <major|minor|patch>   (non-interactive bump)
#        bash scripts/bump-version.sh custom <x.y.z>        (non-interactive custom)
#        bash scripts/bump-version.sh <x.y.z>               (shorthand for custom)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CARGO_TOML="$SCRIPT_DIR/../cli/Cargo.toml"
CARGO_LOCK="$SCRIPT_DIR/../cli/Cargo.lock"
SKILLS_DIR="$SCRIPT_DIR/../skills"
PACKAGE_JSON="$SCRIPT_DIR/../package.json"
PLUGIN_JSONS=(
  "$SCRIPT_DIR/../.claude-plugin/plugin.json"
  "$SCRIPT_DIR/../.cursor-plugin/plugin.json"
  "$SCRIPT_DIR/../.codex-plugin/plugin.json"
)

# ── helpers ──────────────────────────────────────────────────────────────────

bold()  { printf '\033[1m%s\033[0m' "$*"; }
green() { printf '\033[32m%s\033[0m' "$*"; }
cyan()  { printf '\033[36m%s\033[0m' "$*"; }
dim()   { printf '\033[2m%s\033[0m' "$*"; }
red()   { printf '\033[31m%s\033[0m' "$*"; }

if [[ ! -f "$CARGO_TOML" ]]; then
  echo "$(red "Error: Cargo.toml not found: $CARGO_TOML")" >&2
  exit 1
fi

# semver: three numeric parts + optional -prerelease and +build metadata
SEMVER_RE='^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$'

# ── read current version (first `version = "x.y.z"` line — the [package] one) ──

CURRENT="$(grep -m1 -E '^version = "[0-9]+\.[0-9]+\.[0-9]+' "$CARGO_TOML" \
  | sed -E 's/^version = "([^"]+)".*/\1/')"

if [[ -z "$CURRENT" ]]; then
  echo "$(red "Error: could not find a 'version = \"x.y.z\"' line in $CARGO_TOML")" >&2
  exit 1
fi

# parse the numeric core (ignoring any -prerelease / +build suffix) for bumping
CORE="${CURRENT%%[-+]*}"
IFS='.' read -r MAJOR MINOR PATCH <<< "$CORE"

# ── compute the three candidate versions for the prompt ────────────────────────

NEXT_MAJOR="$((MAJOR + 1)).0.0"
NEXT_MINOR="$MAJOR.$((MINOR + 1)).0"
NEXT_PATCH="$MAJOR.$MINOR.$((PATCH + 1))"

# ── pick the bump type: arg if given, else interactive prompt ──────────────────

PART="${1:-}"
CUSTOM="${2:-}"

# shorthand: a bare version (e.g. 3.2.0 or 3.2.0-beta) as first arg means custom
if [[ "$PART" =~ $SEMVER_RE ]]; then
  CUSTOM="$PART"
  PART="custom"
fi

if [[ -z "$PART" ]]; then
  echo
  cyan "bump version — select the version bump type"
  echo
  echo "  Current version: $(bold "$CURRENT")"
  echo
  printf '  1) %s  PATCH  — backwards-compatible bug fixes      → %s\n' "patch " "$(green "$NEXT_PATCH")"
  printf '  2) %s  MINOR  — backwards-compatible new features   → %s\n' "minor " "$(green "$NEXT_MINOR")"
  printf '  3) %s  MAJOR  — incompatible API changes            → %s\n' "major " "$(green "$NEXT_MAJOR")"
  printf '  4) %s  CUSTOM — enter an explicit version\n' "custom"
  echo
  printf '%s' "$(dim 'Enter 1 / 2 / 3 / 4 (or patch / minor / major / custom): ')"
  read -r choice
  case "$choice" in
    1|patch)  PART="patch" ;;
    2|minor)  PART="minor" ;;
    3|major)  PART="major" ;;
    4|custom) PART="custom" ;;
    *) echo "$(red "Invalid choice: '${choice}'. Aborted.")" >&2; exit 1 ;;
  esac
fi

if [[ "$PART" == "custom" && -z "$CUSTOM" ]]; then
  printf '%s' "$(dim "Enter the new version (x.y.z, suffix like -beta allowed), current is ${CURRENT}: ")"
  read -r CUSTOM
fi

case "$PART" in
  major)  MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0; NEW="$MAJOR.$MINOR.$PATCH" ;;
  minor)  MINOR=$((MINOR + 1)); PATCH=0; NEW="$MAJOR.$MINOR.$PATCH" ;;
  patch)  PATCH=$((PATCH + 1)); NEW="$MAJOR.$MINOR.$PATCH" ;;
  custom)
    if [[ ! "$CUSTOM" =~ $SEMVER_RE ]]; then
      echo "$(red "Error: invalid version: '${CUSTOM}' (expected x.y.z, optionally -prerelease / +build)")" >&2
      exit 1
    fi
    NEW="$CUSTOM"
    ;;
  *) echo "$(red "Error: invalid bump type: '${PART}' (expected major|minor|patch|custom)")" >&2; exit 1 ;;
esac

# ── rewrite helpers ────────────────────────────────────────────────────────────

# replace the first `version = "x.y.z"` line (the [package] one) in a Cargo.toml
update_cargo_toml() {
  local file="$1" tmp
  tmp="$(mktemp)"
  awk -v new="$NEW" '
    !done && /^version = "[0-9]+\.[0-9]+\.[0-9]+[^"]*"/ {
      sub(/"[0-9]+\.[0-9]+\.[0-9]+[^"]*"/, "\"" new "\"")
      done = 1
    }
    { print }
  ' "$file" > "$tmp"
  mv "$tmp" "$file"
}

# replace the `version = "x.y.z"` line that follows `name = "onchainos-cli"` in Cargo.lock
update_cargo_lock() {
  local file="$1" tmp
  tmp="$(mktemp)"
  awk -v new="$NEW" '
    /^name = "onchainos-cli"$/ { hit = 1 }
    hit && /^version = "[0-9]+\.[0-9]+\.[0-9]+[^"]*"/ {
      sub(/"[0-9]+\.[0-9]+\.[0-9]+[^"]*"/, "\"" new "\"")
      hit = 0
    }
    { print }
  ' "$file" > "$tmp"
  mv "$tmp" "$file"
}

# replace the first frontmatter `version: "x.y.z"` line in a SKILL.md
update_skill_md() {
  local file="$1" tmp
  tmp="$(mktemp)"
  awk -v new="$NEW" '
    !done && /^[[:space:]]*version:[[:space:]]*"[0-9]+\.[0-9]+\.[0-9]+[^"]*"/ {
      sub(/"[0-9]+\.[0-9]+\.[0-9]+[^"]*"/, "\"" new "\"")
      done = 1
    }
    { print }
  ' "$file" > "$tmp"
  mv "$tmp" "$file"
}

# replace the first top-level `"version": "x.y.z"` line in a JSON manifest
# (package.json / *-plugin/plugin.json)
update_json_version() {
  local file="$1" tmp
  tmp="$(mktemp)"
  awk -v new="$NEW" '
    !done && /^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"[0-9]+\.[0-9]+\.[0-9]+[^"]*"/ {
      sub(/"[0-9]+\.[0-9]+\.[0-9]+[^"]*"/, "\"" new "\"")
      done = 1
    }
    { print }
  ' "$file" > "$tmp"
  mv "$tmp" "$file"
}

# ── apply the new version everywhere ───────────────────────────────────────────

update_cargo_toml "$CARGO_TOML"

LOCK_UPDATED=false
if [[ -f "$CARGO_LOCK" ]] && grep -q '^name = "onchainos-cli"$' "$CARGO_LOCK"; then
  update_cargo_lock "$CARGO_LOCK"
  LOCK_UPDATED=true
fi

SKILL_COUNT=0
if [[ -d "$SKILLS_DIR" ]]; then
  for skill in "$SKILLS_DIR"/*/SKILL.md; do
    [[ -f "$skill" ]] || continue
    if grep -qE '^[[:space:]]*version:[[:space:]]*"[0-9]+\.[0-9]+\.[0-9]+[^"]*"' "$skill"; then
      update_skill_md "$skill"
      SKILL_COUNT=$((SKILL_COUNT + 1))
    fi
  done
fi

PACKAGE_UPDATED=false
if [[ -f "$PACKAGE_JSON" ]] && grep -qE '^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"[0-9]+\.[0-9]+\.[0-9]+[^"]*"' "$PACKAGE_JSON"; then
  update_json_version "$PACKAGE_JSON"
  PACKAGE_UPDATED=true
fi

PLUGIN_COUNT=0
for plugin in "${PLUGIN_JSONS[@]}"; do
  [[ -f "$plugin" ]] || continue
  if grep -qE '^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"[0-9]+\.[0-9]+\.[0-9]+[^"]*"' "$plugin"; then
    update_json_version "$plugin"
    PLUGIN_COUNT=$((PLUGIN_COUNT + 1))
  fi
done

# ── report ─────────────────────────────────────────────────────────────────────

echo
cyan "bump version ($PART)"
echo
echo "  $(bold "$CURRENT")  →  $(green "$NEW")"
echo "  cli/Cargo.toml updated"
if [[ "$LOCK_UPDATED" == true ]]; then
  echo "  cli/Cargo.lock updated (onchainos-cli)"
else
  echo "  $(dim "cli/Cargo.lock skipped (onchainos-cli entry not found)")"
fi
echo "  $SKILL_COUNT SKILL.md file(s) updated"
if [[ "$PACKAGE_UPDATED" == true ]]; then
  echo "  package.json updated"
else
  echo "  $(dim "package.json skipped (version field not found)")"
fi
echo "  $PLUGIN_COUNT plugin.json file(s) updated"
echo

#!/usr/bin/env bash
# sync-shared.sh — interactively copy _shared/ files from okx-agentic-wallet to other skills
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SKILLS_DIR="$SCRIPT_DIR/../skills"
SOURCE_SKILL="okx-agentic-wallet"
SOURCE_DIR="$SKILLS_DIR/$SOURCE_SKILL/_shared"

# ── helpers ──────────────────────────────────────────────────────────────────

bold()  { printf '\033[1m%s\033[0m' "$*"; }
green() { printf '\033[32m%s\033[0m' "$*"; }
cyan()  { printf '\033[36m%s\033[0m' "$*"; }
dim()   { printf '\033[2m%s\033[0m' "$*"; }

# ── sanity check ─────────────────────────────────────────────────────────────

if [[ ! -d "$SOURCE_DIR" ]]; then
  echo "Error: source directory not found: $SOURCE_DIR" >&2
  exit 1
fi

# ── collect available files (bash 3 compatible) ───────────────────────────────

ALL_FILES=()
while IFS= read -r f; do
  ALL_FILES+=("$f")
done < <(ls "$SOURCE_DIR")

if [[ ${#ALL_FILES[@]} -eq 0 ]]; then
  echo "No files found in $SOURCE_DIR" >&2
  exit 1
fi

# ── collect target skills ─────────────────────────────────────────────────────

ALL_SKILLS=()
while IFS= read -r s; do
  [[ "$s" == "$SOURCE_SKILL" ]] && continue
  ALL_SKILLS+=("$s")
done < <(ls "$SKILLS_DIR" | sort)

if [[ ${#ALL_SKILLS[@]} -eq 0 ]]; then
  echo "No other skills found under $SKILLS_DIR" >&2
  exit 1
fi

# ── multi-select helper ───────────────────────────────────────────────────────
# Usage: multi_select <result_var_prefix> <label> item1 item2 ...
# Writes results into global array MULTI_SELECT_RESULT.

MULTI_SELECT_RESULT=()

multi_select() {
  local label=$1
  shift
  local items=("$@")
  local n=${#items[@]}
  local i

  echo
  bold "$label"
  echo
  for i in "${!items[@]}"; do
    printf '  %2d) %s\n' "$((i+1))" "${items[$i]}"
  done
  printf '   a) all\n'
  echo
  printf '%s' "$(dim 'Enter numbers separated by spaces, or a for all: ')"
  read -r raw

  MULTI_SELECT_RESULT=()

  if [[ "$raw" == "a" || "$raw" == "A" ]]; then
    MULTI_SELECT_RESULT=("${items[@]}")
    return
  fi

  for tok in $raw; do
    if [[ "$tok" =~ ^[0-9]+$ ]] && (( tok >= 1 && tok <= n )); then
      MULTI_SELECT_RESULT+=("${items[$((tok-1))]}")
    else
      printf '  Skipping invalid input: %s\n' "$tok" >&2
    fi
  done

  if [[ ${#MULTI_SELECT_RESULT[@]} -eq 0 ]]; then
    echo "No valid selection made. Exiting." >&2
    exit 1
  fi
}

# ── interactive selection ─────────────────────────────────────────────────────

echo
cyan "sync-shared — copy _shared/ from $SOURCE_SKILL to other skills"
echo

multi_select "Which files do you want to copy?" "${ALL_FILES[@]}"
chosen_files=("${MULTI_SELECT_RESULT[@]}")

multi_select "Which skills do you want to copy to?" "${ALL_SKILLS[@]}"
chosen_skills=("${MULTI_SELECT_RESULT[@]}")

# ── confirm ───────────────────────────────────────────────────────────────────

echo
bold "Summary"
echo
echo "  Files : ${chosen_files[*]}"
echo "  Skills: ${chosen_skills[*]}"
echo
printf '%s' "$(dim 'Proceed? [y/N] ')"
read -r confirm
if [[ "$confirm" != "y" && "$confirm" != "Y" ]]; then
  echo "Aborted."
  exit 0
fi

# ── copy ──────────────────────────────────────────────────────────────────────

echo
for skill in "${chosen_skills[@]}"; do
  target_dir="$SKILLS_DIR/$skill/_shared"
  mkdir -p "$target_dir"
  for file in "${chosen_files[@]}"; do
    cp "$SOURCE_DIR/$file" "$target_dir/$file"
    printf '  %s  %s/_shared/%s\n' "$(green '✓')" "$skill" "$file"
  done
done

echo
green "Done."
echo

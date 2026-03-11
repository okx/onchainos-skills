#!/usr/bin/env bash
set -euo pipefail

# ============================================================
# Skill Description Routing Test Runner
# ============================================================
# Tests which skill gets triggered for various user prompts.
# Supports Uniswap (global), Jupiter (global), OKX DEX (custom dir).
#
# IMPORTANT: Run this script directly in your terminal, NOT inside
# a Claude Code session (nested claude invocations are not allowed).
#
# Usage:
#   ./run.sh                                  # run all cases
#   ./run.sh --okx-dir /path/to/okx-skills    # use custom OKX skills
#   ./run.sh --filter T01,T02,T03             # run specific cases
#   ./run.sh --category clear_intent          # run one category
#   ./run.sh --dry-run                        # preview without running
#   ./run.sh --model sonnet                   # specify model
# ============================================================

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CASES_FILE="$SCRIPT_DIR/test-cases.json"
RESULTS_DIR="$SCRIPT_DIR/results"
RAW_DIR="$RESULTS_DIR/raw"
CLEANUP_DIRS=("")

# Defaults
OKX_DIR=""
FILTER=""
CATEGORY=""
DRY_RUN=false
MODEL="sonnet"
MAX_TURNS=3
PARALLEL=5

# ── Cleanup on exit ─────────────────────────────────────────
cleanup() {
  for d in "${CLEANUP_DIRS[@]}"; do
    rm -rf "$d" 2>/dev/null || true
  done
}
trap cleanup EXIT

# ── Parse arguments ──────────────────────────────────────────
usage() {
  cat <<'EOF'
Skill Description Routing Test Runner

Usage: run.sh [OPTIONS]

Options:
  --okx-dir <path>      Path to directory containing OKX DEX skills
                        (e.g. a dir with okx-dex-balance/, okx-dex-market/ etc.)
  --filter <ids>        Comma-separated case IDs to run (e.g. T01,T05,T27)
  --category <name>     Run only this category
                        (clear_intent|vague_intent|competitive|brand|
                         negative|compound|chain|edge|adversarial|multi_turn)
  --model <model>       Claude model to use (default: sonnet)
  --max-turns <n>       Max agentic turns per test (default: 3)
  --parallel <n>        Max parallel claude processes (default: 5)
  --dry-run             Preview test cases without executing
  -h, --help            Show this help

Examples:
  ./run.sh --okx-dir ../okx-dex --category brand
  ./run.sh --filter T27,T28,T29 --model opus
  ./run.sh --parallel 8 --okx-dir ../okx-dex
  ./run.sh --dry-run
EOF
  exit 0
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --okx-dir)    OKX_DIR="$2";    shift 2 ;;
    --filter)     FILTER="$2";     shift 2 ;;
    --category)   CATEGORY="$2";   shift 2 ;;
    --model)      MODEL="$2";      shift 2 ;;
    --max-turns)  MAX_TURNS="$2";  shift 2 ;;
    --parallel)   PARALLEL="$2";   shift 2 ;;
    --dry-run)    DRY_RUN=true;    shift ;;
    -h|--help)    usage ;;
    *)            echo "Unknown option: $1"; usage ;;
  esac
done

# ── Preflight checks ────────────────────────────────────────
for cmd in claude jq; do
  if ! command -v "$cmd" &>/dev/null; then
    echo "Error: '$cmd' is required but not found in PATH." >&2
    exit 1
  fi
done

if [[ ! -f "$CASES_FILE" ]]; then
  echo "Error: test-cases.json not found at $CASES_FILE" >&2
  exit 1
fi

# If running inside Claude Code, we'll unset CLAUDECODE for child processes
if [[ -n "${CLAUDECODE:-}" ]]; then
  echo ">> Running inside Claude Code session, child processes will bypass nested check."
fi

mkdir -p "$RAW_DIR"

# ── Setup OKX skills via plugin-dir ──────────────────────────
# --plugin-dir expects: <dir>/skills/<skill-name>/SKILL.md
# OKX dir has:          <dir>/<skill-name>/SKILL.md
# So we create a temp wrapper with a "skills" symlink.
# ── Target skills (only these count in results) ─────────────
# Uniswap
TARGET_UNISWAP="swap-planner,swap-integration,liquidity-planner,v4-security-foundations,viem-integration,configurator,deployer"
# Jupiter
TARGET_JUPITER="integrating-jupiter"
# OKX DEX (new)
TARGET_OKX="okx-wallet-portfolio,okx-dex-market,okx-dex-swap,okx-dex-token,okx-onchain-gateway"
# All targets combined
ALL_TARGETS=",$TARGET_UNISWAP,$TARGET_JUPITER,$TARGET_OKX,"

CLAUDE_BASE_ARGS=(
  --print
  --verbose
  --output-format stream-json
  --model "$MODEL"
  --max-turns "$MAX_TURNS"
  --no-session-persistence
)

if [[ -n "$OKX_DIR" ]]; then
  # Resolve to absolute path
  OKX_DIR="$(cd "$OKX_DIR" && pwd)"
  if [[ ! -d "$OKX_DIR" ]]; then
    echo "Error: OKX dir not found: $OKX_DIR" >&2
    exit 1
  fi

  # Verify it contains SKILL.md files
  skill_count=$(find "$OKX_DIR" -name "SKILL.md" -maxdepth 2 | wc -l | tr -d ' ')
  if [[ "$skill_count" -eq 0 ]]; then
    echo "Error: No SKILL.md files found in $OKX_DIR" >&2
    exit 1
  fi

  # Create wrapper directory for --plugin-dir
  OKX_PLUGIN_DIR=$(mktemp -d)
  CLEANUP_DIRS+=("$OKX_PLUGIN_DIR")
  ln -s "$OKX_DIR" "$OKX_PLUGIN_DIR/skills"

  CLAUDE_BASE_ARGS+=(--plugin-dir "$OKX_PLUGIN_DIR")
  echo ">> OKX skills loaded from: $OKX_DIR ($skill_count skills found)"
fi

# ── Skill detection from stream-json ─────────────────────────
# Claude stream-json emits tool_use blocks when skills are invoked.
# We parse for the Skill tool call and extract the skill name.
detect_skills() {
  local raw_file="$1"
  local all_skills=""

  # Method 1: Skill tool invocation — "skill":"swap-planner"
  # Strip plugin namespace prefix (e.g. "tmp.xxx:okx-dex-balance" → "okx-dex-balance")
  all_skills=$(grep -oE '"skill"[[:space:]]*:[[:space:]]*"[^"]+"' "$raw_file" \
    | sed 's/.*"skill"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/' \
    | sed 's/^[^:]*://' \
    | sort -u \
    | paste -sd ',' - 2>/dev/null || true)

  if [[ -z "$all_skills" ]]; then
    # Method 2: Look for Skill in tool_use name field
    if grep -qE '"name"[[:space:]]*:[[:space:]]*"Skill"' "$raw_file" 2>/dev/null; then
      all_skills="__unknown__"
    fi
  fi

  if [[ -z "$all_skills" ]]; then
    # Method 3: Look for skill loading message patterns
    all_skills=$(grep -oE 'Loading skill ['\''"`]([^'\''"` ]+)' "$raw_file" \
      | sed 's/Loading skill ['\''"`]*//' \
      | sort -u \
      | paste -sd ',' - 2>/dev/null || true)
  fi

  if [[ -z "$all_skills" ]]; then
    echo "__none__"
    return
  fi

  # Filter: only keep target skills (Uniswap / Jupiter / OKX-new)
  local filtered=""
  local old_ifs="$IFS"
  IFS=','
  for s in $all_skills; do
    if [[ "$ALL_TARGETS" == *",$s,"* ]]; then
      if [[ -z "$filtered" ]]; then
        filtered="$s"
      else
        filtered="$filtered,$s"
      fi
    fi
  done
  IFS="$old_ifs"

  if [[ -z "$filtered" ]]; then
    echo "__none__"
  else
    echo "$filtered"
  fi
}

# ── Evaluate a single result ─────────────────────────────────
evaluate() {
  local triggered="$1"
  local expected="$2"
  local expected_not="$3"

  # __any__ = anything is acceptable
  if [[ "$expected" == "__any__" ]]; then
    echo "correct"
    return
  fi

  # Negative test: expect no skill triggered
  if [[ "$expected" == "__none__" ]]; then
    if [[ "$triggered" == "__none__" ]]; then
      echo "correct"
    else
      echo "false_positive"
    fi
    return
  fi

  # Check expected_not first (should NOT have triggered)
  if [[ -n "$expected_not" ]]; then
    local old_ifs="$IFS"
    IFS=','
    local not_arr=($expected_not)
    local trig_arr=($triggered)
    IFS="$old_ifs"
    for ns in "${not_arr[@]}"; do
      for ts in "${trig_arr[@]}"; do
        if [[ "$ts" == "$ns" ]]; then
          echo "wrong"
          return
        fi
      done
    done
  fi

  # No skill triggered but we expected one
  if [[ "$triggered" == "__none__" ]]; then
    echo "missed"
    return
  fi

  # Check if any triggered skill is in expected list
  local old_ifs="$IFS"
  IFS=','
  local trig_arr=($triggered)
  local exp_arr=($expected)
  IFS="$old_ifs"
  for ts in "${trig_arr[@]}"; do
    for es in "${exp_arr[@]}"; do
      if [[ "$ts" == "$es" ]]; then
        echo "correct"
        return
      fi
    done
  done

  echo "wrong"
}

# ── Parallel job control (bash 3 compatible) ─────────────────
wait_for_jobs() {
  local max_jobs="$1"
  while true; do
    local running
    running=$(jobs -rp | wc -l | tr -d ' ')
    if [[ "$running" -lt "$max_jobs" ]]; then
      break
    fi
    sleep 0.3
  done
}

# ── Run a single-turn test case ──────────────────────────────
# Writes result to individual file: $RESULTS_DIR/result_${id}.json
run_single() {
  local id="$1"
  local prompt="$2"
  local expected="$3"
  local expected_not="${4:-}"
  local raw_file="$RAW_DIR/${id}.jsonl"
  local result_file="$RESULTS_DIR/result_${id}.json"

  if $DRY_RUN; then
    echo "  [$id] $prompt ... (dry run, expected: $expected)"
    return
  fi

  # Run claude in a clean session
  echo "$prompt" | env -u CLAUDECODE claude "${CLAUDE_BASE_ARGS[@]}" \
    > "$raw_file" 2>&1 || true

  local triggered
  triggered=$(detect_skills "$raw_file")
  local verdict
  verdict=$(evaluate "$triggered" "$expected" "$expected_not")

  # Write individual result file (compact single-line JSON)
  jq -cn \
    --arg id "$id" \
    --arg prompt "$prompt" \
    --arg triggered "$triggered" \
    --arg expected "$expected" \
    --arg expected_not "$expected_not" \
    --arg verdict "$verdict" \
    '{id:$id, prompt:$prompt, triggered:$triggered, expected:$expected, expected_not:$expected_not, verdict:$verdict}' \
    > "$result_file"

  # Print status (may interleave with other parallel jobs, that's fine)
  case "$verdict" in
    correct)        echo -e "  [$id] \033[32m✓ $verdict\033[0m  (triggered: $triggered)" ;;
    missed)         echo -e "  [$id] \033[33m○ $verdict\033[0m  (triggered: $triggered, expected: $expected)" ;;
    wrong)          echo -e "  [$id] \033[31m✗ $verdict\033[0m  (triggered: $triggered, expected: $expected)" ;;
    false_positive) echo -e "  [$id] \033[31m✗ $verdict\033[0m  (triggered: $triggered, expected: none)" ;;
  esac
}

# ── Run a multi-turn test case ───────────────────────────────
run_multi_turn() {
  local id="$1"
  local turns_json="$2"
  local note="${3:-}"
  local turn_count
  turn_count=$(echo "$turns_json" | jq 'length')

  echo "  [$id] Multi-turn ($turn_count turns)${note:+ — $note}"

  if $DRY_RUN; then
    echo "$turns_json" | jq -r '.[] | "    Turn: \(.prompt) → expected: \(.expected | join(","))"'
    return
  fi

  local session_id=""
  local turn_idx=0

  while [[ $turn_idx -lt $turn_count ]]; do
    local turn
    turn=$(echo "$turns_json" | jq ".[$turn_idx]")
    local prompt
    prompt=$(echo "$turn" | jq -r '.prompt')
    local expected
    expected=$(echo "$turn" | jq -r '.expected | join(",")')
    local expected_not
    expected_not=$(echo "$turn" | jq -r 'if .expected_not then .expected_not | join(",") else "" end')

    local raw_file="$RAW_DIR/${id}_turn${turn_idx}.jsonl"
    local turn_id="${id}.${turn_idx}"

    echo "    Turn $turn_idx: $prompt"

    # First turn: fresh session (without --no-session-persistence so we can resume)
    # Subsequent turns: resume the session
    local turn_args=(
      --print
      --verbose
      --output-format stream-json
      --model "$MODEL"
      --max-turns "$MAX_TURNS"
    )

    # Add plugin-dir if configured
    for arg in "${CLAUDE_BASE_ARGS[@]}"; do
      if [[ "$arg" == "--plugin-dir" ]]; then
        turn_args+=(--plugin-dir)
      elif [[ -n "${prev_was_plugin:-}" ]]; then
        turn_args+=("$arg")
        prev_was_plugin=""
      fi
      [[ "$arg" == "--plugin-dir" ]] && prev_was_plugin=1
    done

    if [[ -n "$session_id" ]]; then
      turn_args+=(--resume "$session_id")
    fi

    claude "${turn_args[@]}" "$prompt" \
      > "$raw_file" 2>&1 || true

    # Extract session_id from first turn for continuation
    if [[ -z "$session_id" ]]; then
      session_id=$(grep -oE '"sessionId"[[:space:]]*:[[:space:]]*"[^"]+"' "$raw_file" \
        | head -1 \
        | sed 's/.*"\([0-9a-f-]*\)"$/\1/' || true)
      # Fallback pattern
      if [[ -z "$session_id" ]]; then
        session_id=$(grep -oE '"session_id"[[:space:]]*:[[:space:]]*"[^"]+"' "$raw_file" \
          | head -1 \
          | sed 's/.*"\([0-9a-f-]*\)"$/\1/' || true)
      fi
    fi

    local triggered
    triggered=$(detect_skills "$raw_file")
    local verdict
    verdict=$(evaluate "$triggered" "$expected" "$expected_not")

    case "$verdict" in
      correct)        echo -e "    Turn $turn_idx: \033[32m✓ $verdict\033[0m  (triggered: $triggered)" ;;
      missed)         echo -e "    Turn $turn_idx: \033[33m○ $verdict\033[0m  (triggered: $triggered, expected: $expected)" ;;
      wrong)          echo -e "    Turn $turn_idx: \033[31m✗ $verdict\033[0m  (triggered: $triggered, expected: $expected)" ;;
      false_positive) echo -e "    Turn $turn_idx: \033[31m✗ $verdict\033[0m  (triggered: $triggered, expected: none)" ;;
    esac

    local result_file="$RESULTS_DIR/result_${turn_id}.json"
    jq -cn \
      --arg id "$turn_id" \
      --arg prompt "$prompt" \
      --arg triggered "$triggered" \
      --arg expected "$expected" \
      --arg expected_not "$expected_not" \
      --arg verdict "$verdict" \
      '{id:$id, prompt:$prompt, triggered:$triggered, expected:$expected, expected_not:$expected_not, verdict:$verdict}' \
      > "$result_file"

    turn_idx=$((turn_idx + 1))
  done
}

# ── Generate markdown report ─────────────────────────────────
generate_report() {
  local results_file="$RESULTS_DIR/results.jsonl"
  local report_file="$RESULTS_DIR/report.md"

  if [[ ! -f "$results_file" ]] || [[ ! -s "$results_file" ]]; then
    echo "No results to report."
    return
  fi

  local total correct wrong missed false_positive
  total=$(wc -l < "$results_file" | tr -d ' ')
  correct=$(grep -c '"correct"' "$results_file" || true)
  wrong=$(grep -c '"wrong"' "$results_file" || true)
  missed=$(grep -c '"missed"' "$results_file" || true)
  false_positive=$(grep -c '"false_positive"' "$results_file" || true)

  local recall="N/A" precision="N/A"
  if [[ $((correct + missed)) -gt 0 ]]; then
    recall=$(echo "scale=1; $correct * 100 / ($correct + $missed)" | bc)
  fi
  if [[ $((correct + wrong + false_positive)) -gt 0 ]]; then
    precision=$(echo "scale=1; $correct * 100 / ($correct + $wrong + $false_positive)" | bc)
  fi

  # Write report header
  cat > "$report_file" <<HEADER
# Skill Routing Test Report

- **Date**: $(date '+%Y-%m-%d %H:%M:%S')
- **Model**: $MODEL
- **Max Turns**: $MAX_TURNS
- **OKX Dir**: ${OKX_DIR:-"(global)"}
- **Total Cases**: $total

## Summary

| Metric | Value |
|---|---|
| Correct | $correct |
| Wrong | $wrong |
| Missed | $missed |
| False Positive | $false_positive |
| **Recall** | **${recall}%** |
| **Precision** | **${precision}%** |

## By Category

| Category | Correct / Total | Rate |
|---|---|---|
HEADER

  # Per-category breakdown
  for cat_name in clear_intent vague_intent competitive brand negative compound chain edge adversarial multi_turn pnl_routing memepump signals token_deep_dive gateway; do
    local ids
    ids=$(jq -r ".cases[] | select(.category == \"$cat_name\") | .id" "$CASES_FILE" 2>/dev/null | paste -sd '|' -)
    if [[ -z "$ids" ]]; then continue; fi

    # For multi_turn, IDs in results have .N suffix
    local cat_total cat_correct
    if [[ "$cat_name" == "multi_turn" ]]; then
      cat_total=$(grep -cE "\"id\":\"($ids)\\." "$results_file" || true)
      cat_correct=$(grep -E "\"id\":\"($ids)\\." "$results_file" 2>/dev/null \
        | grep -c '"correct"' || true)
    else
      cat_total=$(grep -cE "\"id\":\"($ids)\"" "$results_file" || true)
      cat_correct=$(grep -E "\"id\":\"($ids)\"" "$results_file" 2>/dev/null \
        | grep -c '"correct"' || true)
    fi

    if [[ "$cat_total" -gt 0 ]]; then
      local cat_rate
      cat_rate=$(echo "scale=1; $cat_correct * 100 / $cat_total" | bc)
      echo "| $cat_name | $cat_correct / $cat_total | ${cat_rate}% |" >> "$report_file"
    fi
  done

  # Competitive win rate
  cat >> "$report_file" <<'SECTION'

## Competitive Win Rate

Skills triggered in competitive-intent cases (no brand specified):

| Skill | Times Triggered |
|---|---|
SECTION

  local comp_ids
  comp_ids=$(jq -r '.cases[] | select(.category == "competitive") | .id' "$CASES_FILE" | paste -sd '|' -)
  if [[ -n "$comp_ids" ]]; then
    grep -E "\"id\":\"($comp_ids)\"" "$results_file" 2>/dev/null \
      | jq -r '.triggered' \
      | tr ',' '\n' \
      | grep -v '^__' \
      | sort \
      | uniq -c \
      | sort -rn \
      | awk '{print "| " $2 " | " $1 " |"}' \
      >> "$report_file" 2>/dev/null || true
  fi

  # Brand routing accuracy
  cat >> "$report_file" <<'SECTION'

## Brand Routing

| ID | Prompt | Expected | Triggered | Verdict |
|---|---|---|---|---|
SECTION

  local brand_ids
  brand_ids=$(jq -r '.cases[] | select(.category == "brand") | .id' "$CASES_FILE" | paste -sd '|' -)
  if [[ -n "$brand_ids" ]]; then
    grep -E "\"id\":\"($brand_ids)\"" "$results_file" 2>/dev/null \
      | jq -r '[.id, .prompt, .expected, .triggered, .verdict] | @tsv' \
      | while IFS=$'\t' read -r fid fprompt fexpected ftriggered fverdict; do
          echo "| $fid | $fprompt | $fexpected | $ftriggered | $fverdict |" >> "$report_file"
        done || true
  fi

  # All failures
  cat >> "$report_file" <<'SECTION'

## All Failures

| ID | Prompt | Expected | Triggered | Verdict |
|---|---|---|---|---|
SECTION

  grep -v '"correct"' "$results_file" 2>/dev/null \
    | jq -r '[.id, .prompt, .expected, .triggered, .verdict] | @tsv' \
    | while IFS=$'\t' read -r fid fprompt fexpected ftriggered fverdict; do
        echo "| $fid | $fprompt | $fexpected | $ftriggered | $fverdict |" >> "$report_file"
      done 2>/dev/null || true

  echo ""
  echo ">> Report saved to: $report_file"
}

# ── Main ─────────────────────────────────────────────────────
echo "============================================================"
echo " Skill Routing Test Runner"
echo " Model: $MODEL | Max Turns: $MAX_TURNS | Parallel: $PARALLEL"
echo " Cases: $CASES_FILE"
[[ -n "$OKX_DIR" ]] && echo " OKX Dir: $OKX_DIR"
[[ -n "$FILTER" ]] && echo " Filter: $FILTER"
[[ -n "$CATEGORY" ]] && echo " Category: $CATEGORY"
echo "============================================================"
echo ""

# Clear previous results
> "$RESULTS_DIR/results.jsonl"
rm -f "$RESULTS_DIR"/result_*.json

# Build filter CSV (bash 3 compatible, no associative arrays)
FILTER_CSV=""
if [[ -n "$FILTER" ]]; then
  FILTER_CSV=",$FILTER,"
fi

# Collect ordered IDs for merging later
ORDERED_IDS=""

# Iterate cases — single-turn run in parallel, multi-turn run serially
case_count=$(jq '.cases | length' "$CASES_FILE")
run_count=0
skip_count=0

for i in $(seq 0 $((case_count - 1))); do
  case_json=$(jq ".cases[$i]" "$CASES_FILE")
  id=$(echo "$case_json" | jq -r '.id')
  category=$(echo "$case_json" | jq -r '.category')

  # Apply filters
  if [[ -n "$FILTER_CSV" ]] && [[ "$FILTER_CSV" != *",$id,"* ]]; then
    skip_count=$((skip_count + 1))
    continue
  fi
  if [[ -n "$CATEGORY" ]] && [[ "$category" != "$CATEGORY" ]]; then
    skip_count=$((skip_count + 1))
    continue
  fi

  run_count=$((run_count + 1))

  if [[ "$category" == "multi_turn" ]]; then
    # Wait for all parallel single-turn jobs before starting multi-turn
    wait
    turns=$(echo "$case_json" | jq '.turns')
    note=$(echo "$case_json" | jq -r '.note // empty')
    turn_count=$(echo "$turns" | jq 'length')
    for ti in $(seq 0 $((turn_count - 1))); do
      ORDERED_IDS="$ORDERED_IDS ${id}.${ti}"
    done
    run_multi_turn "$id" "$turns" "$note"
  else
    ORDERED_IDS="$ORDERED_IDS $id"
    prompt=$(echo "$case_json" | jq -r '.prompt')
    expected=$(echo "$case_json" | jq -r '.expected | join(",")')
    expected_not=$(echo "$case_json" | jq -r 'if .expected_not then .expected_not | join(",") else "" end')

    if $DRY_RUN; then
      run_single "$id" "$prompt" "$expected" "$expected_not"
    else
      # Wait if we've hit the parallel limit
      wait_for_jobs "$PARALLEL"
      echo "  [$id] started ..."
      run_single "$id" "$prompt" "$expected" "$expected_not" &
    fi
  fi
done

# Wait for remaining parallel jobs
wait
echo ""

# Merge individual result files in order → results.jsonl
for rid in $ORDERED_IDS; do
  result_file="$RESULTS_DIR/result_${rid}.json"
  if [[ -f "$result_file" ]]; then
    cat "$result_file" >> "$RESULTS_DIR/results.jsonl"
  fi
done

echo "──────────────────────────────────────────────────────────"
echo " Done. Ran $run_count cases, skipped $skip_count."
echo "──────────────────────────────────────────────────────────"

if ! $DRY_RUN; then
  generate_report
fi

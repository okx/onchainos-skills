#!/usr/bin/env bash
# Test a candidate onchainos binary together with the beta skills package and
# the current okx-a2a package.
#
# Everything is installed under a temporary HOME. The caller's global CLI,
# skills, credentials, and ~/.onchainos state are never modified.

set -euo pipefail

# Update-test inputs. Keep these values in the script so the LLM only needs to
# run `bash scripts/test-onchainos-update.sh`.
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "$script_dir/.." && pwd -P)"
candidate_binary="$repo_root/.codex/build/cargo-target/debug/onchainos"
beta_package="okx/onchainos-skills#beta"
a2a_package="@okxweb3/a2a-node@latest"
keep_sandbox=false
post_update_args=()

usage() {
  cat <<'EOF'
Usage:
  scripts/test-onchainos-update.sh [options] [-- <onchainos args...>]

Options:
  --keep               Keep the isolated test directory after the run.
  -h, --help           Show this help.

Built-in inputs:
  onchainos:  .codex/build/cargo-target/debug/onchainos
  skills:     okx/onchainos-skills#beta
  okx-a2a:    @okxweb3/a2a-node@latest

The script:
  1. copies the candidate binary into an isolated ~/.local/bin;
  2. runs `npx skills add <package> --yes -g` in an isolated HOME;
  3. installs and verifies the requested okx-a2a package;
  4. reads the installed okx-agentic-wallet skill version;
  5. runs `onchainos preflight --skill-version ... --no-self-update`;
  6. follows the data.action gate: only when it is null does the optional
     command after `--` run in a new process.

Examples:
  scripts/test-onchainos-update.sh

  scripts/test-onchainos-update.sh -- agent --help

The optional command uses isolated state and therefore has no real login.
Do not use it for an authenticated create-task/create-subscribe smoke test;
after this update test passes, retry that command separately with the normal
CLI so it uses the user's existing credentials.
EOF
}

die() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

while (($#)); do
  case "$1" in
    --keep)
      keep_sandbox=true
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      post_update_args=("$@")
      break
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

[[ -f "$candidate_binary" ]] || die "candidate binary not found: $candidate_binary"
[[ -x "$candidate_binary" ]] || die "candidate binary is not executable: $candidate_binary"
[[ -n "$beta_package" ]] || die "--package must not be empty"
[[ -n "$a2a_package" ]] || die "--a2a-package must not be empty"
command -v npx >/dev/null 2>&1 || die "npx is required"
command -v npm >/dev/null 2>&1 || die "npm is required"
command -v node >/dev/null 2>&1 || die "node is required"

test_root="$(mktemp -d "${TMPDIR:-/tmp}/onchainos-update-test.XXXXXX")"
test_home="$test_root/home"
test_bin_dir="$test_home/.local/bin"
test_cli="$test_bin_dir/onchainos"
test_onchainos_home="$test_home/.onchainos"
test_npm_cache="$test_root/npm-cache"
test_xdg_config="$test_home/.config"
test_xdg_data="$test_home/.local/share"
install_log="$test_root/npx-skills-add.log"
a2a_install_log="$test_root/npm-a2a-install.log"

cleanup() {
  if [[ "$keep_sandbox" == true ]]; then
    printf 'Kept isolated test directory: %s\n' "$test_root"
  else
    rm -rf -- "$test_root"
  fi
}
trap cleanup EXIT

mkdir -p "$test_bin_dir" "$test_onchainos_home" "$test_npm_cache" \
  "$test_xdg_config" "$test_xdg_data"
chmod 700 "$test_home" "$test_bin_dir" "$test_onchainos_home"
cp -- "$candidate_binary" "$test_cli"
chmod 700 "$test_cli"

candidate_version="$($test_cli --version 2>/dev/null)" \
  || die "candidate binary failed to run: $candidate_binary"
printf 'Candidate CLI: %s\n' "$candidate_version"
printf 'Beta package: %s\n' "$beta_package"
printf 'OKX A2A package: %s\n' "$a2a_package"
printf 'Isolated HOME: %s\n' "$test_home"

printf 'Installing beta skills...\n'
set +e
HOME="$test_home" \
XDG_CONFIG_HOME="$test_xdg_config" \
XDG_DATA_HOME="$test_xdg_data" \
npm_config_cache="$test_npm_cache" \
npm_config_fetch_retries=1 \
npm_config_fetch_timeout=30000 \
PATH="$test_bin_dir:$PATH" \
  npx skills add "$beta_package" --yes -g >"$install_log" 2>&1
npx_status=$?
set -e

wallet_skill=""
for candidate in \
  "$test_home/.agents/skills/okx-agentic-wallet/SKILL.md" \
  "$test_home/.codex/skills/okx-agentic-wallet/SKILL.md" \
  "$test_home/.claude/skills/okx-agentic-wallet/SKILL.md" \
  "$test_home/.openclaw/skills/okx-agentic-wallet/SKILL.md" \
  "$test_home/.cursor/skills/okx-agentic-wallet/SKILL.md"
do
  if [[ -f "$candidate" ]]; then
    wallet_skill="$candidate"
    break
  fi
done

if [[ -z "$wallet_skill" ]]; then
  sed -n '1,200p' "$install_log" >&2
  die "beta skills installation did not produce okx-agentic-wallet/SKILL.md"
fi

if ((npx_status != 0)); then
  if grep -Fq 'PromptScript does not support global skill installation' "$install_log"; then
    printf 'Warning: ignored the known PromptScript global-install error; skill files exist.\n'
  else
    sed -n '1,200p' "$install_log" >&2
    die "npx skills add failed with exit code $npx_status"
  fi
fi

printf 'Installing OKX A2A...\n'
if ! HOME="$test_home" \
  XDG_CONFIG_HOME="$test_xdg_config" \
  XDG_DATA_HOME="$test_xdg_data" \
  npm_config_cache="$test_npm_cache" \
  npm_config_prefix="$test_home/.local" \
  npm_config_fetch_retries=1 \
  npm_config_fetch_timeout=30000 \
  PATH="$test_bin_dir:$PATH" \
    npm install -g "$a2a_package" >"$a2a_install_log" 2>&1
then
  sed -n '1,200p' "$a2a_install_log" >&2
  die "okx-a2a package installation failed"
fi

[[ -x "$test_bin_dir/okx-a2a" ]] || die "okx-a2a executable was not installed"
a2a_version="$(
  HOME="$test_home" \
  XDG_CONFIG_HOME="$test_xdg_config" \
  XDG_DATA_HOME="$test_xdg_data" \
  PATH="$test_bin_dir:$PATH" \
    "$test_bin_dir/okx-a2a" --version
)" || die "installed okx-a2a failed to run"
printf 'Installed OKX A2A: %s\n' "$a2a_version"

skill_version="$(awk '
  /^metadata:[[:space:]]*$/ { in_metadata=1; next }
  in_metadata && /^[^[:space:]]/ { exit }
  in_metadata && /^[[:space:]]+version:[[:space:]]*/ {
    sub(/^[[:space:]]+version:[[:space:]]*/, "")
    gsub(/^['\''\"]|['\''\"]$/, "")
    print
    exit
  }
' "$wallet_skill")"
[[ -n "$skill_version" ]] || die "could not read metadata.version from $wallet_skill"
printf 'Installed wallet skill: %s\n' "$skill_version"

# Do not let preflight replace the candidate during this verification. The
# supplied candidate is the exact new package under test.
preflight_json="$(
  HOME="$test_home" \
  XDG_CONFIG_HOME="$test_xdg_config" \
  XDG_DATA_HOME="$test_xdg_data" \
  ONCHAINOS_HOME="$test_onchainos_home" \
  ONCHAINOS_CREDENTIAL_STORE=file \
  ONCHAINOS_SKIP_CLIENT_VERSION_GATE=false \
  PATH="$test_bin_dir:$PATH" \
    "$test_cli" preflight --skill-version "$skill_version" --no-self-update
)" || die "candidate preflight failed"

action="$(node -e '
  const fs = require("fs");
  const input = fs.readFileSync(0, "utf8");
  let payload;
  try { payload = JSON.parse(input); }
  catch (error) { console.error(`invalid preflight JSON: ${error.message}`); process.exit(2); }
  if (payload.ok !== true || !payload.data) {
    console.error("preflight did not return {ok:true,data:{...}}");
    process.exit(3);
  }
  const action = payload.data.action;
  if (action !== null && typeof action !== "string") {
    console.error("preflight data.action must be null or a string");
    process.exit(4);
  }
  process.stdout.write(action || "");
' <<<"$preflight_json")" || die "could not validate preflight output"

if [[ -n "$action" ]]; then
  printf 'Preflight requires action; optional retry was not run:\n%s\n' "$action" >&2
  exit 1
fi

printf 'Preflight passed: data.action is null.\n'

if ((${#post_update_args[@]})); then
  printf 'Running optional post-update command in a new isolated CLI process.\n'
  HOME="$test_home" \
  XDG_CONFIG_HOME="$test_xdg_config" \
  XDG_DATA_HOME="$test_xdg_data" \
  ONCHAINOS_HOME="$test_onchainos_home" \
  ONCHAINOS_CREDENTIAL_STORE=file \
  ONCHAINOS_SKIP_CLIENT_VERSION_GATE=false \
  PATH="$test_bin_dir:$PATH" \
    "$test_cli" "${post_update_args[@]}"
fi

printf 'PASS: candidate CLI, beta skills, and okx-a2a passed the isolated update check.\n'
printf 'Repository under test: %s\n' "$repo_root"

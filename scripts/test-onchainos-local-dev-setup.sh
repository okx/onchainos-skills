#!/usr/bin/env bash
# Verify setup against a globally installed okx-a2a substitute and no local A2A repo.

set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "$script_dir/.." && pwd -P)"
temp_root="$(mktemp -d "${TMPDIR:-/tmp}/onchainos-local-setup.XXXXXX")"
cleanup() { rm -rf "$temp_root"; }
trap cleanup EXIT

fixture_root="$temp_root/project"
fake_bin="$temp_root/global-bin"
mkdir -p "$fixture_root/cli" "$fixture_root/skills/demo" "$fixture_root/scripts" "$fake_bin"
fixture_root="$(cd -- "$fixture_root" && pwd -P)"
printf '%s\n' '[package]' 'name = "fixture"' 'version = "0.0.0"' > "$fixture_root/cli/Cargo.toml"
printf '%s\n' '---' 'name: demo' '---' '# Demo' > "$fixture_root/skills/demo/SKILL.md"
cp "$repo_root/scripts/onchainos-local-dev-setup.sh" "$fixture_root/scripts/"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  'set -euo pipefail' \
  ': "${CARGO_TARGET_DIR:?}"' \
  ': "${OKX_BASE_URL:?}"' \
  'mkdir -p "$CARGO_TARGET_DIR/debug"' \
  'printf "%s\\n" "$OKX_BASE_URL" > "$CARGO_TARGET_DIR/observed-base-url"' \
  'printf "%s\\n" "#!/usr/bin/env bash" '\''printf "tmp=%s base_url=%s version_gate=%s local-onchainos %s\\n" "$TMPDIR" "$OKX_BASE_URL" "$ONCHAINOS_SKIP_CLIENT_VERSION_GATE" "$*"'\'' > "$CARGO_TARGET_DIR/debug/onchainos"' \
  'chmod 700 "$CARGO_TARGET_DIR/debug/onchainos"' \
  > "$fake_bin/cargo"
chmod 700 "$fake_bin/cargo"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  'set -euo pipefail' \
  'if [[ "${1:-}" == "daemon" && ( "${2:-}" == "start" || "${2:-}" == "restart" ) ]]; then' \
  '  rm -rf "${OKX_AGENT_TASK_HOME:?}/workspace"' \
  '  mkdir -p "$OKX_AGENT_TASK_HOME/workspace"' \
  '  printf "%s\\n" "${TMPDIR:?}" > "$OKX_AGENT_TASK_HOME/observed-tmpdir"' \
  'fi' \
  'printf "global-okx-a2a %s\\n" "$*"' \
  > "$fake_bin/okx-a2a"
chmod 700 "$fake_bin/okx-a2a"

env -u OKX_BASE_URL PATH="$fake_bin:$PATH" bash "$fixture_root/scripts/onchainos-local-dev-setup.sh" init

wrapper_output="$("$fixture_root/.codex/bin/onchainos" probe)"
[[ "$wrapper_output" == "tmp=$fixture_root/.codex/runtime/tmp base_url=https://forked-walletmain-swim.okx.testokg.com version_gate=true local-onchainos probe" ]]
[[ "$(< "$fixture_root/.codex/build/cargo-target/observed-base-url")" == "https://forked-walletmain-swim.okx.testokg.com" ]]
preflight_output="$("$fixture_root/.codex/bin/onchainos" preflight --skill-version 0.0.0)"
[[ "$preflight_output" == '{"ok":true,"data":{"status":"skipped","preflightSkipped":true,"skipReason":"project-local-wrapper","action":null}}' ]]
[[ "$("$fixture_root/.codex/bin/okx-a2a" --version)" == "global-okx-a2a --version" ]]
[[ -L "$fixture_root/.codex/skills/demo" ]]
[[ ! -e "$fixture_root/a2a" ]]
! grep -q 'ONCHAINOS_SKIP_PREFLIGHT' "$fixture_root/.codex/bin/onchainos"
! grep -q 'ONCHAINOS_SKIP_PREFLIGHT' "$fixture_root/.codex/bin/okx-a2a"
grep -qx 'export ONCHAINOS_SKIP_CLIENT_VERSION_GATE="${ONCHAINOS_SKIP_CLIENT_VERSION_GATE:-true}"' "$fixture_root/.codex/bin/onchainos"
grep -qx 'export ONCHAINOS_SKIP_CLIENT_VERSION_GATE="${ONCHAINOS_SKIP_CLIENT_VERSION_GATE:-true}"' "$fixture_root/.codex/bin/okx-a2a"
grep -qx "export OKX_BASE_URL='https://forked-walletmain-swim.okx.testokg.com'" "$fixture_root/.codex/bin/onchainos"
grep -qx "export OKX_BASE_URL='https://forked-walletmain-swim.okx.testokg.com'" "$fixture_root/.codex/bin/okx-a2a"
grep -qx "export TMPDIR='$fixture_root/.codex/runtime/tmp'" "$fixture_root/.codex/bin/onchainos"
grep -qx "export TMPDIR='$fixture_root/.codex/runtime/tmp'" "$fixture_root/.codex/bin/okx-a2a"
grep -qx "export ONCHAINOS_A2A_SPOOL_DIR='$fixture_root/.codex/runtime/a2a-spool'" "$fixture_root/.codex/bin/onchainos"
grep -qx "export ONCHAINOS_A2A_SPOOL_DIR='$fixture_root/.codex/runtime/a2a-spool'" "$fixture_root/.codex/bin/okx-a2a"
[[ "$(< "$fixture_root/.codex/runtime/a2a/observed-tmpdir")" == "$fixture_root/.codex/runtime/tmp" ]]
[[ "$(stat -f '%Lp' "$fixture_root/.codex/runtime/tmp")" == "700" ]]
[[ "$(stat -f '%Lp' "$fixture_root/.codex/runtime/a2a-spool")" == "700" ]]

workspace_dir="$fixture_root/.codex/runtime/a2a/workspace"
[[ -L "$workspace_dir/.codex/bin/onchainos" ]]
[[ -L "$workspace_dir/.codex/bin/okx-a2a" ]]
[[ -L "$workspace_dir/.codex/skills" ]]
[[ "$(cd "$workspace_dir" && ./.codex/bin/onchainos probe)" == "tmp=$fixture_root/.codex/runtime/tmp base_url=https://forked-walletmain-swim.okx.testokg.com version_gate=true local-onchainos probe" ]]

"$fixture_root/.codex/bin/okx-a2a" daemon restart >/dev/null
[[ -L "$workspace_dir/.codex/bin/onchainos" ]]
[[ -L "$workspace_dir/.codex/skills" ]]
[[ "$(< "$fixture_root/.codex/runtime/a2a/observed-tmpdir")" == "$fixture_root/.codex/runtime/tmp" ]]
[[ "$(cd "$workspace_dir" && ./.codex/bin/onchainos probe)" == "tmp=$fixture_root/.codex/runtime/tmp base_url=https://forked-walletmain-swim.okx.testokg.com version_gate=true local-onchainos probe" ]]

mkdir -p "$temp_root/external-skill"
printf '%s\n' '---' 'name: external' '---' '# External' > "$temp_root/external-skill/SKILL.md"
ln -s "$temp_root/external-skill" "$fixture_root/.codex/skills/external"
rm -rf "$fixture_root/skills/demo"
bash "$fixture_root/scripts/onchainos-local-dev-setup.sh" skills >/dev/null
[[ ! -e "$fixture_root/.codex/skills/demo" ]]
[[ -L "$fixture_root/.codex/skills/external" ]]

printf 'PASS: project-local setup works with only a globally installed okx-a2a.\n'

# scripts/preflight.ps1 — onchainos session preflight (Windows PowerShell).
#
# Ships inside each skill's scripts/ folder. Invoked from SKILL.md or
# support files via a relative path from the skill root:
# `powershell scripts/preflight.ps1 -SkillVersion ...`.
#
# Pipeline:
#   1. Ensure the onchainos CLI is available.
#   2. Defer the skill-version drift check to `onchainos skills check`.

param([string]$SkillVersion = "")

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($SkillVersion)) {
  [Console]::Error.WriteLine("warn: preflight.ps1 missing -SkillVersion arg")
  exit 2
}

if (-not (Get-Command onchainos -ErrorAction SilentlyContinue)) {
  [Console]::Error.WriteLine("warn: onchainos CLI is not installed. Install it from https://github.com/okx/onchainos-skills#installation")
  exit 2
}

onchainos skills check --expected-version=$SkillVersion
exit $LASTEXITCODE

# scripts/preflight.ps1 — onchainos session preflight (Windows PowerShell).
#
# Ships inside each skill's scripts/ folder. Invoked from SKILL.md via
# $env:CLAUDE_SKILL_DIR\scripts\preflight.ps1 (Claude Code substitutes the
# variable to the skill's absolute install path at load time). On other
# agents, the caller must provide an equivalent absolute-path mechanism.
#
# Pipeline:
#   1. Resolve the latest stable release tag (12h cache at $env:USERPROFILE\.onchainos\last_check).
#   2. Install or update the CLI via install.ps1 if missing or out-of-date.
#   3. Defer the skill-version drift check to `onchainos skills check`.

param([Parameter(Mandatory=$true)][string]$SkillVersion)

$ErrorActionPreference = "Stop"

$Repo = "okx/onchainos-skills"
$CacheDir = Join-Path $env:USERPROFILE ".onchainos"
$CacheFile = Join-Path $CacheDir "last_check"
$CacheTTL = 43200  # 12h
$InstallUrl = "https://raw.githubusercontent.com/$Repo/main/install.ps1"

New-Item -ItemType Directory -Force -Path $CacheDir | Out-Null

$latestTag = $null
$now = [int][double]::Parse((Get-Date -UFormat %s))
if (Test-Path $CacheFile) {
  $mtime = [int][double]::Parse((((Get-Item $CacheFile).LastWriteTimeUtc) - (Get-Date "1970-01-01Z")).TotalSeconds.ToString())
  if (($now - $mtime) -lt $CacheTTL) {
    $latestTag = (Get-Content $CacheFile | Select-Object -Last 1)
  }
}
if (-not $latestTag) {
  try {
    $release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest"
    $latestTag = $release.tag_name
    if ($latestTag) { "$now`n$latestTag" | Out-File -FilePath $CacheFile -Encoding ascii }
  } catch { }
}

if (-not (Get-Command onchainos -ErrorAction SilentlyContinue)) {
  & ([scriptblock]::Create((Invoke-RestMethod $InstallUrl)))
} elseif ($latestTag) {
  $installed = "v" + ((onchainos --version) -split ' ' | Select-Object -Last 1)
  if ($installed -ne $latestTag) {
    & ([scriptblock]::Create((Invoke-RestMethod $InstallUrl)))
  }
}

onchainos skills check --expected-version=$SkillVersion

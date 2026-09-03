# ──────────────────────────────────────────────────────────────
# onchainos installer / updater (Windows)
#
# Usage (stable):
#   irm https://raw.githubusercontent.com/okx/onchainos-skills/main/install.ps1 | iex
#
# Usage (beta):
#   $env:ONCHAINOS_BETA=1; irm https://raw.githubusercontent.com/okx/onchainos-skills/main/install.ps1 | iex
#   # or
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/okx/onchainos-skills/main/install.ps1))) --beta
#
# Behavior:
#   - Default (stable): fetches latest stable release from GitHub,
#     compares with local version, installs/upgrades if needed.
#   - Beta: fetches all tags, finds the latest version (including
#     pre-releases) by semver, and installs it.
#   - Every invocation checks the requested release channel. Automatic
#     update throttling is handled by `onchainos preflight` / `upgrade --throttle`.
#
# Supported platforms:
#   Windows: x86_64, i686, ARM64
# ──────────────────────────────────────────────────────────────

param(
    [switch]$beta
)

$ErrorActionPreference = "Stop"

# Force TLS 1.2 for Windows PowerShell 5.1 on older Windows builds, where the
# default security protocol may exclude it, causing HTTPS calls to github.com to
# fail with "Could not create SSL/TLS secure channel". -bor adds TLS 1.2 without
# disabling TLS 1.3 where it is already enabled. Harmless no-op on PowerShell 7+.
try {
    [Net.ServicePointManager]::SecurityProtocol = `
        [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch {}

# Support --beta via remaining args (PowerShell treats -- as param terminator)
if ($args -contains "beta" -or $args -contains "--beta") {
    $beta = [switch]::new($true)
}
# Support ONCHAINOS_BETA env var (for irm | iex which cannot pass args)
if ($env:ONCHAINOS_BETA) {
    $beta = [switch]::new($true)
}

$REPO = "okx/onchainos-skills"
$BINARY = "onchainos"
$INSTALL_DIR = Join-Path $env:USERPROFILE ".local\bin"
$CACHE_DIR = Join-Path $env:USERPROFILE ".onchainos"

function Get-Target {
    $arch = $env:PROCESSOR_ARCHITECTURE
    switch ($arch) {
        "AMD64"  { return "x86_64-pc-windows-msvc" }
        "x86"    { return "i686-pc-windows-msvc" }
        "ARM64"  { return "aarch64-pc-windows-msvc" }
        default  { throw "Unsupported architecture: $arch" }
    }
}

# ── Version helpers ──────────────────────────────────────────
function Get-LocalVersion {
    $binaryPath = Join-Path $INSTALL_DIR "$BINARY.exe"
    if (Test-Path $binaryPath) {
        $output = & $binaryPath --version 2>$null
        if ($output -match "\S+\s+(\S+)") { return $Matches[1] }
    }
    return $null
}

function Get-BaseVersion([string]$ver) {
    return ($ver -split '-')[0]
}

function Get-PreRelease([string]$ver) {
    if ($ver -match '-(.+)$') { return $Matches[1] }
    return $null
}

# Semver greater-than: returns $true if $v1 > $v2
function Test-SemverGt([string]$v1, [string]$v2) {
    $base1 = Get-BaseVersion $v1
    $base2 = Get-BaseVersion $v2
    $pre1 = Get-PreRelease $v1
    $pre2 = Get-PreRelease $v2

    $parts1 = $base1 -split '\.'
    $parts2 = $base2 -split '\.'

    for ($i = 0; $i -lt 3; $i++) {
        $f1 = if ($parts1[$i]) { [int]$parts1[$i] } else { 0 }
        $f2 = if ($parts2[$i]) { [int]$parts2[$i] } else { 0 }
        if ($f1 -gt $f2) { return $true }
        if ($f1 -lt $f2) { return $false }
    }

    # Base versions equal — compare pre-release
    if (-not $pre1 -and -not $pre2) { return $false }  # equal
    if (-not $pre1) { return $true }   # stable > any pre-release
    if (-not $pre2) { return $false }  # pre-release < stable

    # Both have pre-release (e.g., beta.0 vs beta.1)
    $num1 = if ($pre1 -match '(\d+)$') { [int]$Matches[1] } else { 0 }
    $num2 = if ($pre2 -match '(\d+)$') { [int]$Matches[1] } else { 0 }
    return ($num1 -gt $num2)
}

# ── GitHub API helpers ───────────────────────────────────────

# Call the GitHub API. Honors $env:GITHUB_TOKEN when set (raises the rate limit
# from 60/hr to 5000/hr). Only used as a fallback — the primary version-lookup
# paths below avoid api.github.com entirely.
function Invoke-GitHubApi([string]$Uri) {
    $headers = @{}
    if ($env:GITHUB_TOKEN) { $headers["Authorization"] = "Bearer $env:GITHUB_TOKEN" }
    return Invoke-RestMethod -Uri $Uri -TimeoutSec 10 -UseBasicParsing -Headers $headers
}

# Fetch latest stable version.
# Primary path follows the /releases/latest redirect, which is served by the
# github.com website backend and does NOT count against the 60/hr unauthenticated
# API limit. Reads the final URL from BaseResponse — ResponseUri on Windows
# PowerShell 5.1, RequestMessage.RequestUri on PowerShell 7+. Falls back to the
# releases API if the redirect can't be parsed.
function Get-LatestStableVersion {
    try {
        $resp = Invoke-WebRequest -Uri "https://github.com/${REPO}/releases/latest" `
            -Method Head -MaximumRedirection 5 -TimeoutSec 10 -UseBasicParsing
        $base = $resp.BaseResponse
        $final = $null
        if (($base.PSObject.Properties['ResponseUri']) -and $base.ResponseUri) {
            $final = $base.ResponseUri.AbsoluteUri              # Windows PowerShell 5.1
        } elseif ($base.RequestMessage) {
            $final = $base.RequestMessage.RequestUri.AbsoluteUri # PowerShell 7+
        }
        if ($final -match '/tag/v(.+)$') { return $Matches[1] }
    } catch {}

    try {
        $response = Invoke-GitHubApi "https://api.github.com/repos/${REPO}/releases/latest"
        $ver = $response.tag_name -replace '^v', ''
        if ($ver) { return $ver }
    } catch {}

    throw "Could not fetch latest version from GitHub. Check your network connection or install manually from https://github.com/${REPO}"
}

# Fetch latest version including betas.
# Primary path lists tags via git smart-http (git ls-remote), which does NOT
# count against the API limit. Falls back to the tags API if git is unavailable
# or fails. Returns the highest by semver using Test-SemverGt (which correctly
# orders pre-releases below their base version).
function Get-LatestVersionWithBeta {
    $versions = @()

    if (Get-Command git -ErrorAction SilentlyContinue) {
        try {
            # GIT_HTTP_LOW_SPEED_* aborts a stalled transfer (proxy/firewall) so
            # the API fallback can run; GIT_TERMINAL_PROMPT=0 prevents a hang on
            # an auth prompt. Saved/restored to avoid leaking into the session.
            $oldPrompt = $env:GIT_TERMINAL_PROMPT
            $oldLimit  = $env:GIT_HTTP_LOW_SPEED_LIMIT
            $oldTime   = $env:GIT_HTTP_LOW_SPEED_TIME
            $env:GIT_TERMINAL_PROMPT = "0"
            $env:GIT_HTTP_LOW_SPEED_LIMIT = "1000"
            $env:GIT_HTTP_LOW_SPEED_TIME = "15"
            try {
                $lines = git ls-remote --tags "https://github.com/${REPO}.git" 2>$null
            } finally {
                $env:GIT_TERMINAL_PROMPT = $oldPrompt
                $env:GIT_HTTP_LOW_SPEED_LIMIT = $oldLimit
                $env:GIT_HTTP_LOW_SPEED_TIME = $oldTime
            }
            foreach ($line in $lines) {
                $ref = (($line -split "/")[-1]) -replace '\^\{\}', ''
                if ($ref -match '^v[0-9]') { $versions += ($ref -replace '^v', '') }
            }
            $versions = @($versions | Sort-Object -Unique)
        } catch {}
    }

    if (-not $versions -or $versions.Count -eq 0) {
        try {
            $response = Invoke-GitHubApi "https://api.github.com/repos/${REPO}/tags?per_page=100"
            foreach ($tag in $response) {
                $v = $tag.name -replace '^v', ''
                if ($v) { $versions += $v }
            }
        } catch {}
    }

    if (-not $versions -or $versions.Count -eq 0) {
        throw "Could not fetch tags from GitHub. Check your network connection or install manually from https://github.com/${REPO}"
    }

    $best = $null
    foreach ($v in $versions) {
        if (-not $best -or (Test-SemverGt $v $best)) {
            $best = $v
        }
    }
    if (-not $best) { throw "No valid versions found in tags." }
    return $best
}

# ── Binary installer ─────────────────────────────────────────
function Install-Binary {
    param([string]$Tag)

    $target = Get-Target
    $binaryName = "${BINARY}-${target}.exe"
    $url = "https://github.com/${REPO}/releases/download/${Tag}/${binaryName}"
    $checksumsUrl = "https://github.com/${REPO}/releases/download/${Tag}/checksums.txt"

    Write-Host "Installing ${BINARY} ${Tag} (${target})..."

    $tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

    try {
        $binaryPath = Join-Path $tmpDir $binaryName
        $checksumsPath = Join-Path $tmpDir "checksums.txt"

        Invoke-WebRequest -Uri $url -OutFile $binaryPath -UseBasicParsing
        Invoke-WebRequest -Uri $checksumsUrl -OutFile $checksumsPath -UseBasicParsing

        $expectedLine = Get-Content $checksumsPath | Where-Object { $_ -match $binaryName } | Select-Object -First 1
        if (-not $expectedLine) { throw "No checksum found for $binaryName" }
        $expectedHash = ($expectedLine -split "\s+")[0]

        $actualHash = (Get-FileHash -Path $binaryPath -Algorithm SHA256).Hash.ToLower()

        if ($actualHash -ne $expectedHash) {
            throw @"
Checksum mismatch!
  Expected: $expectedHash
  Got:      $actualHash
The downloaded file may have been tampered with. Aborting.
"@
        }

        Write-Host "Checksum verified."

        if (-not (Test-Path $INSTALL_DIR)) { New-Item -ItemType Directory -Path $INSTALL_DIR -Force | Out-Null }
        $destPath = Join-Path $INSTALL_DIR "$BINARY.exe"
        Move-Item -Path $binaryPath -Destination $destPath -Force

        Write-Host "Installed ${BINARY} ${Tag} to ${destPath}"
    }
    finally {
        Remove-Item -Path $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

# ── Workflow sync ────────────────────────────────────────────
function Sync-Workflows {
    param([string]$Tag)

    $workflowsDir = Join-Path $CACHE_DIR "workflows"
    $workflowsUrl = "https://github.com/${REPO}/releases/download/${Tag}/workflows.tar.gz"
    $checksumsUrl = "https://github.com/${REPO}/releases/download/${Tag}/workflows-checksums.txt"

    Write-Host "Syncing workflows (${Tag})..."

    $tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

    try {
        $archivePath = Join-Path $tmpDir "workflows.tar.gz"
        Invoke-WebRequest -Uri $workflowsUrl -OutFile $archivePath -UseBasicParsing -TimeoutSec 30

        # Verify checksum — fail closed: skip install if verification cannot complete
        $checksumsPath = Join-Path $tmpDir "workflows-checksums.txt"
        try {
            Invoke-WebRequest -Uri $checksumsUrl -OutFile $checksumsPath -UseBasicParsing -TimeoutSec 10
        } catch {
            Write-Host "Warning: could not download workflows checksum — skipping (non-fatal)" -ForegroundColor Yellow
            return
        }

        $expectedLine = Get-Content $checksumsPath | Where-Object { $_ -match "workflows.tar.gz" } | Select-Object -First 1
        if (-not $expectedLine) {
            Write-Host "Warning: no checksum found for workflows.tar.gz — skipping (non-fatal)" -ForegroundColor Yellow
            return
        }
        $expectedHash = ($expectedLine -split "\s+")[0]
        $actualHash = (Get-FileHash -Path $archivePath -Algorithm SHA256).Hash.ToLower()
        if ($actualHash -ne $expectedHash) {
            Write-Host "Warning: workflows checksum mismatch — skipping (non-fatal)" -ForegroundColor Yellow
            return
        }

        tar -xzf "$archivePath" -C "$tmpDir" 2>$null
        if ($LASTEXITCODE -ne 0) {
            Write-Host "Warning: could not extract workflows (non-fatal)" -ForegroundColor Yellow
            return
        }

        $srcWorkflows = Join-Path $tmpDir "workflows"
        if (Test-Path $srcWorkflows) {
            if (Test-Path $workflowsDir) { Remove-Item -Path $workflowsDir -Recurse -Force }
            if (-not (Test-Path $CACHE_DIR)) { New-Item -ItemType Directory -Path $CACHE_DIR -Force | Out-Null }
            Move-Item -Path $srcWorkflows -Destination $workflowsDir -Force
            Write-Host "Workflows synced to ${workflowsDir}"
        }
    }
    catch {
        Write-Host "Warning: could not sync workflows ($($_.Exception.Message)) (non-fatal)" -ForegroundColor Yellow
    }
    finally {
        Remove-Item -Path $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

# ── PATH setup ───────────────────────────────────────────────
function Add-ToPath {
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($userPath -split ";" | Where-Object { $_ -eq $INSTALL_DIR }) { return }

    $newPath = "${INSTALL_DIR};${userPath}"
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    $env:Path = "${INSTALL_DIR};${env:Path}"

    Write-Host ""
    Write-Host "Added $INSTALL_DIR to your user PATH."
    Write-Host "Restart your terminal or run the following to use '${BINARY}' now:"
    Write-Host ""
    Write-Host "  `$env:Path = `"${INSTALL_DIR};`$env:Path`""
    Write-Host ""
}

# ── Optional companion tools ─────────────────────────────────────────────────────
# okx-a2a environment readiness is owned by `okx-a2a doctor --fix`; this hook
# installs the latest CLI package and hands the rest to doctor. Every step is
# best-effort and silent on failure.
function Get-A2ACommandPath {
    foreach ($commandName in @("okx-a2a.cmd", "okx-a2a.exe", "okx-a2a")) {
        $command = Get-Command $commandName -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($command -and $command.Path) { return $command.Path }
    }
    return $null
}

function Get-A2AVersion([string]$CommandPath) {
    if (-not $CommandPath) { return $null }
    try {
        $output = & $CommandPath --version 2>$null
        return (($output | Out-String).Trim())
    } catch {
        return $null
    }
}

function Finish-Install {
    $npmCommand = Get-Command npm.cmd -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $npmCommand) {
        $npmCommand = Get-Command npm -ErrorAction SilentlyContinue | Select-Object -First 1
    }
    if (-not $npmCommand) { return }

    Write-Host ""
    Write-Host "Installing the latest okx-a2a and ensuring the A2A environment (non-fatal)..."

    $a2aCommandPath = Get-A2ACommandPath
    $beforeVer = Get-A2AVersion -CommandPath $a2aCommandPath
    try { & $npmCommand.Path install -g "@okxweb3/a2a-node@latest" *> $null } catch {}

    # A running daemon keeps executing the old code after the package directory
    # is replaced, so restart it when the installed version actually changed.
    $a2aCommandPath = Get-A2ACommandPath
    $afterVer = Get-A2AVersion -CommandPath $a2aCommandPath
    if ($afterVer -and $afterVer -ne $beforeVer) {
        try {
            $daemonStatus = & $a2aCommandPath daemon status 2>$null | Select-Object -First 1
            if ("$daemonStatus" -match '^running') {
                try { & $a2aCommandPath daemon restart *> $null } catch {}
            }
        } catch {}
    }

    if ($a2aCommandPath) {
        try { & $a2aCommandPath doctor --fix --non-interactive *> $null } catch {}
    }
    Write-Host "okx-a2a environment check completed."
}

# ── Main ─────────────────────────────────────────────────────
function Main {
    $localVer = Get-LocalVersion

    if ($beta) {
        # ── Beta mode: find latest version including pre-releases ──
        $targetVer = Get-LatestVersionWithBeta

        # Never replace a newer local beta/dev build with an older remote tag.
        # Update only when the resolved target is strictly newer than local.
        if ($localVer -and -not (Test-SemverGt $targetVer $localVer)) {
            if ($localVer -ne $targetVer) {
                Write-Host "Installed onchainos ${localVer} is newer than the latest published ${targetVer}; keeping local version."
            }
            $wfDir = Join-Path $CACHE_DIR "workflows"
            if (-not (Test-Path $wfDir)) { Sync-Workflows -Tag "v${localVer}" }
            Finish-Install
            return
        }
    } else {
        # ── Stable mode ──

        $latestStable = Get-LatestStableVersion

        if (-not $localVer) {
            # Not installed — install latest stable
            $targetVer = $latestStable
        } elseif ($localVer -eq $latestStable) {
            # Already on exact latest stable
            $wfDir = Join-Path $CACHE_DIR "workflows"
            if (-not (Test-Path $wfDir)) { Sync-Workflows -Tag "v${localVer}" }
            Finish-Install
            return
        } else {
            if (Test-SemverGt $latestStable $localVer) {
                # Latest stable is newer than local (handles beta→stable upgrade too)
                $targetVer = $latestStable
            } else {
                # Local is same or newer (e.g., on a beta ahead of stable)
                $wfDir = Join-Path $CACHE_DIR "workflows"
                if (-not (Test-Path $wfDir)) { Sync-Workflows -Tag "v${localVer}" }
                Finish-Install
                return
            }
        }
    }

    if ($localVer) {
        Write-Host "Updating ${BINARY} from ${localVer} to ${targetVer}..."
    }

    Install-Binary -Tag "v${targetVer}"
    Sync-Workflows -Tag "v${targetVer}"
    Add-ToPath
    Finish-Install
}

Main

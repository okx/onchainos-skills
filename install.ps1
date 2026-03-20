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
#   - Caches the last check timestamp. Skips GitHub API calls if
#     checked within the last 12 hours.
#
# Supported platforms:
#   Windows: x86_64, i686, ARM64
# ──────────────────────────────────────────────────────────────

param(
    [switch]$beta
)

$ErrorActionPreference = "Stop"

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
$CACHE_FILE = Join-Path $CACHE_DIR "last_check"
$CACHE_TTL = 43200  # 12 hours in seconds

function Get-Target {
    $arch = $env:PROCESSOR_ARCHITECTURE
    switch ($arch) {
        "AMD64"  { return "x86_64-pc-windows-msvc" }
        "x86"    { return "i686-pc-windows-msvc" }
        "ARM64"  { return "aarch64-pc-windows-msvc" }
        default  { throw "Unsupported architecture: $arch" }
    }
}

# ── Cache helpers ────────────────────────────────────────────
function Test-CacheFresh {
    if (-not (Test-Path $CACHE_FILE)) { return $false }
    $cachedTs = (Get-Content $CACHE_FILE -ErrorAction SilentlyContinue | Select-Object -First 1).Trim()
    if (-not $cachedTs) { return $false }
    $now = [int][double]::Parse((Get-Date -UFormat %s))
    $elapsed = $now - [int]$cachedTs
    return ($elapsed -lt $CACHE_TTL)
}

function Write-Cache {
    if (-not (Test-Path $CACHE_DIR)) { New-Item -ItemType Directory -Path $CACHE_DIR -Force | Out-Null }
    [int][double]::Parse((Get-Date -UFormat %s)) | Out-File -FilePath $CACHE_FILE -Encoding ascii -NoNewline
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

function Get-LatestStableVersion {
    try {
        $response = Invoke-RestMethod -Uri "https://api.github.com/repos/${REPO}/releases/latest" -TimeoutSec 10 -UseBasicParsing
        $ver = $response.tag_name -replace '^v', ''
        if ($ver) { return $ver }
    } catch {}
    throw "Could not fetch latest version from GitHub. Check your network connection or install manually from https://github.com/${REPO}"
}

function Get-LatestVersionWithBeta {
    try {
        $response = Invoke-RestMethod -Uri "https://api.github.com/repos/${REPO}/tags?per_page=100" -TimeoutSec 10 -UseBasicParsing
        $best = $null
        foreach ($tag in $response) {
            $v = $tag.name -replace '^v', ''
            if (-not $v) { continue }
            if (-not $best -or (Test-SemverGt $v $best)) {
                $best = $v
            }
        }
        if ($best) { return $best }
    } catch {}
    throw "Could not fetch tags from GitHub. Check your network connection or install manually from https://github.com/${REPO}"
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

# ── Main ─────────────────────────────────────────────────────
function Main {
    $localVer = Get-LocalVersion

    if ($beta) {
        # ── Beta mode: find latest version including pre-releases ──
        $targetVer = Get-LatestVersionWithBeta

        if ($localVer -eq $targetVer) {
            Write-Cache
            return
        }
    } else {
        # ── Stable mode ──

        # Fast path: binary exists and was checked recently — skip API call
        if ($localVer -and (Test-CacheFresh)) { return }

        $latestStable = Get-LatestStableVersion

        if (-not $localVer) {
            # Not installed — install latest stable
            $targetVer = $latestStable
        } elseif ($localVer -eq $latestStable) {
            # Already on exact latest stable
            Write-Cache
            return
        } else {
            if (Test-SemverGt $latestStable $localVer) {
                # Latest stable is newer than local (handles beta→stable upgrade too)
                $targetVer = $latestStable
            } else {
                # Local is same or newer (e.g., on a beta ahead of stable)
                Write-Cache
                return
            }
        }
    }

    if ($localVer) {
        Write-Host "Updating ${BINARY} from ${localVer} to ${targetVer}..."
    }

    Install-Binary -Tag "v${targetVer}"
    Write-Cache
    Add-ToPath
}

Main

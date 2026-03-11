# ──────────────────────────────────────────────────────────────
# onchainos installer / updater (Windows)
#
# Usage:
#   irm https://raw.githubusercontent.com/okx/onchainos-skills/main/install.ps1 | iex
#
# Behavior:
#   - Fresh install: detect platform, download the pinned version, verify
#     SHA256 checksum, install.
#   - Already installed: skip if the correct version was verified within the
#     last 12 hours (cache at ~/.onchainos/last_check). Otherwise, confirm the
#     installed version matches REQUIRED_VERSION and reinstall if needed.
#
# Supported platforms:
#   Windows: x86_64, i686, ARM64
# ──────────────────────────────────────────────────────────────

$ErrorActionPreference = "Stop"

$REPO = "okx/onchainos-skills"
$BINARY = "onchainos"
$INSTALL_DIR = Join-Path $env:USERPROFILE ".local\bin"
$CACHE_DIR = Join-Path $env:USERPROFILE ".onchainos"
$CACHE_FILE = Join-Path $CACHE_DIR "last_check"
$CACHE_TTL = 43200  # 12 hours in seconds
$REQUIRED_VERSION = "1.0.3"  # Managed by release workflow — do not edit manually

function Get-Target {
    $arch = $env:PROCESSOR_ARCHITECTURE
    switch ($arch) {
        "AMD64"  { return "x86_64-pc-windows-msvc" }
        "x86"    { return "i686-pc-windows-msvc" }
        "ARM64"  { return "aarch64-pc-windows-msvc" }
        default  { throw "Unsupported architecture: $arch" }
    }
}

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

function Get-LocalVersion {
    $binaryPath = Join-Path $INSTALL_DIR "$BINARY.exe"
    if (Test-Path $binaryPath) {
        $output = & $binaryPath --version 2>$null
        if ($output -match "\S+\s+(\S+)") { return $Matches[1] }
    }
    return $null
}

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

function Main {
    $localVer = Get-LocalVersion
    $tag = "v${REQUIRED_VERSION}"

    # Fast path: correct version already installed and verified recently
    if (($localVer -eq $REQUIRED_VERSION) -and (Test-CacheFresh)) { return }

    # Correct version installed but cache expired — refresh cache
    if ($localVer -eq $REQUIRED_VERSION) {
        Write-Cache
        return
    }

    if ($localVer) {
        Write-Host "Updating ${BINARY} from ${localVer} to ${REQUIRED_VERSION}..."
    }

    Install-Binary -Tag $tag
    Write-Cache
    Add-ToPath
}

Main

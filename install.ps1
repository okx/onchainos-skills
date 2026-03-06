#Requires -Version 5.1
<#
.SYNOPSIS
    One-click installer for the onchainos CLI on Windows.

.DESCRIPTION
    Downloads the latest onchainos binary from GitHub Releases,
    verifies SHA256 checksum, installs to a local user directory, and adds it to PATH.
    Automatically selects the best download method (Invoke-WebRequest / WebClient / curl.exe).

.NOTES
    Usage (single command, works on all Windows 10/11 PowerShell):
      powershell -ExecutionPolicy Bypass -File install.ps1

    Or run directly from the web:
      powershell -ExecutionPolicy Bypass -Command "& { [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; $s = (New-Object Net.WebClient).DownloadString('https://raw.githubusercontent.com/okx/onchainos-skills/main/install.ps1'); Invoke-Expression $s }"

    Supported architectures: x64 / x86 / ARM64
#>

$ErrorActionPreference = "Stop"

# ── Configuration ─────────────────────────────────────────────
$REPO        = "okx/onchainos-skills"
$BINARY      = "onchainos"
$INSTALL_DIR = "$env:LOCALAPPDATA\onchainos\bin"  # Install to user-local directory, no admin required

# ── Download helper: auto-select available method ────────────
# Priority: Invoke-WebRequest → WebClient → curl.exe
# Handles older PowerShell versions that lack irm / Invoke-WebRequest
function Download-File {
    param(
        [string]$Url,
        [string]$OutFile
    )

    # Force TLS 1.2 (compatibility with older Windows PowerShell 5.1)
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

    # Method 1: Invoke-WebRequest (built-in PowerShell 5.1+, most common)
    if (Get-Command Invoke-WebRequest -ErrorAction SilentlyContinue) {
        try {
            Invoke-WebRequest -Uri $Url -OutFile $OutFile -UseBasicParsing
            return
        } catch {
            # If failed, try next method
        }
    }

    # Method 2: System.Net.WebClient (.NET built-in, available on nearly all Windows)
    try {
        (New-Object System.Net.WebClient).DownloadFile($Url, $OutFile)
        return
    } catch {
        # Continue trying
    }

    # Method 3: curl.exe (bundled with Windows 10 1803+)
    $curlExe = Get-Command curl.exe -ErrorAction SilentlyContinue
    if ($curlExe) {
        & curl.exe -sSfL $Url -o $OutFile
        if ($LASTEXITCODE -eq 0) { return }
    }

    throw "No download method available. Please install PowerShell 5.1+ or ensure curl.exe is on PATH."
}

# ── Fetch JSON helper: auto-select available method ──────────────
function Get-JsonFromUrl {
    param([string]$Url)

    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

    # Method 1: Invoke-RestMethod (returns parsed object)
    if (Get-Command Invoke-RestMethod -ErrorAction SilentlyContinue) {
        try {
            return Invoke-RestMethod -Uri $Url
        } catch {}
    }

    # Method 2: WebClient + manual JSON parsing
    try {
        $json = (New-Object System.Net.WebClient).DownloadString($Url)
        return $json | ConvertFrom-Json
    } catch {}

    # Method 3: curl.exe + manual parsing
    $curlExe = Get-Command curl.exe -ErrorAction SilentlyContinue
    if ($curlExe) {
        $json = & curl.exe -sSfL $Url 2>$null
        if ($LASTEXITCODE -eq 0 -and $json) {
            return $json | ConvertFrom-Json
        }
    }

    throw "Failed to fetch $Url. Please check your network connection."
}

# ── Detect CPU architecture ───────────────────────────────────
function Get-Target {
    # RuntimeInformation available in .NET Framework 4.7.1+ and .NET Core
    try {
        $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    } catch {
        # Fallback: detect via environment variable
        $arch = $env:PROCESSOR_ARCHITECTURE
    }

    switch -Regex ($arch) {
        "X64|AMD64"  { return "x86_64-pc-windows-msvc" }
        "X86|x86"    { return "i686-pc-windows-msvc" }
        "Arm64|ARM64" { return "aarch64-pc-windows-msvc" }
        default      { throw "Unsupported CPU architecture: $arch" }
    }
}

# ── Main flow ──────────────────────────────────────────────────
function Main {
    $target = Get-Target

    # Query GitHub API for latest stable release (skip prerelease)
    Write-Host "Detecting latest release..."
    $release = Get-JsonFromUrl "https://api.github.com/repos/$REPO/releases/latest"
    $tag = $release.tag_name
    if (-not $tag) {
        throw "Could not determine latest release"
    }

    $binaryName   = "$BINARY-$target.exe"
    $url          = "https://github.com/$REPO/releases/download/$tag/$binaryName"
    $checksumsUrl = "https://github.com/$REPO/releases/download/$tag/checksums.txt"

    Write-Host "Installing $BINARY $tag ($target)..."

    # Create temporary directory
    $tmpDir = New-Item -ItemType Directory -Path (Join-Path $env:TEMP ([System.IO.Path]::GetRandomFileName()))

    try {
        $binaryPath    = Join-Path $tmpDir $binaryName
        $checksumsPath = Join-Path $tmpDir "checksums.txt"

        # Download binary and checksum files
        Write-Host "Downloading..."
        Download-File -Url $url          -OutFile $binaryPath
        Download-File -Url $checksumsUrl -OutFile $checksumsPath

        # SHA256 verification: ensure downloaded file has not been tampered with
        $checksums    = Get-Content $checksumsPath
        $expectedLine = $checksums | Where-Object { $_ -match [regex]::Escape($binaryName) }
        if (-not $expectedLine) {
            throw "No checksum found for $binaryName"
        }
        $expectedHash = ($expectedLine -split '\s+')[0]
        $actualHash   = (Get-FileHash -Path $binaryPath -Algorithm SHA256).Hash.ToLower()

        if ($actualHash -ne $expectedHash) {
            throw @"
Checksum mismatch! The downloaded file may have been tampered with.
  Expected: $expectedHash
  Got:      $actualHash
"@
        }
        Write-Host "Checksum verified."

        # Install to user-local directory
        if (-not (Test-Path $INSTALL_DIR)) {
            New-Item -ItemType Directory -Path $INSTALL_DIR -Force | Out-Null
        }
        Copy-Item -Path $binaryPath -Destination (Join-Path $INSTALL_DIR "$BINARY.exe") -Force

        # Add install directory to user PATH (if not already present)
        $userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
        if ($userPath -notlike "*$INSTALL_DIR*") {
            [Environment]::SetEnvironmentVariable("PATH", "$INSTALL_DIR;$userPath", "User")
            Write-Host "Added $INSTALL_DIR to user PATH. Restart your terminal for it to take effect."
        }

        Write-Host ""
        Write-Host "Installed $BINARY to $INSTALL_DIR\$BINARY.exe"
        Write-Host "Run '$BINARY --help' to get started."
    }
    finally {
        # Clean up temporary files
        Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue
    }
}

Main

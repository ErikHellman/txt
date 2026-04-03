# Install the latest release of txt to $env:LOCALAPPDATA\txt
# Usage: irm https://raw.githubusercontent.com/<owner>/txt/main/install.ps1 | iex

$ErrorActionPreference = "Stop"

$Repo        = "<owner>/txt"
$BinName     = "txt"
$Target      = "x86_64-pc-windows-msvc"
$InstallDir  = Join-Path $env:LOCALAPPDATA $BinName

# ── Resolve latest version ────────────────────────────────────────────────────
$Release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest"
$Version = $Release.tag_name

if (-not $Version) {
    Write-Error "Could not determine latest version (rate limited?)"
    exit 1
}

$Archive = "$BinName-$Version-$Target.zip"
$Url     = "https://github.com/$Repo/releases/download/$Version/$Archive"

Write-Host "Installing $BinName $Version ..."

# ── Download ──────────────────────────────────────────────────────────────────
$TmpFile = [System.IO.Path]::GetTempFileName() + ".zip"
try {
    Invoke-WebRequest -Uri $Url -OutFile $TmpFile -UseBasicParsing

    # ── Extract ───────────────────────────────────────────────────────────────
    if (Test-Path $InstallDir) { Remove-Item $InstallDir -Recurse -Force }
    New-Item -ItemType Directory -Path $InstallDir | Out-Null
    Expand-Archive -Path $TmpFile -DestinationPath $InstallDir -Force
} finally {
    if (Test-Path $TmpFile) { Remove-Item $TmpFile -Force }
}

Write-Host "Installed to $InstallDir\$BinName.exe"

# ── Add to user PATH if not already present ───────────────────────────────────
$UserPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("PATH", "$UserPath;$InstallDir", "User")
    Write-Host "Added $InstallDir to your PATH (takes effect in new terminals)."
} else {
    Write-Host "$InstallDir is already in your PATH."
}

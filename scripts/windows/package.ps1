param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [switch]$SkipInstaller
)

$ErrorActionPreference = "Stop"
$RootDir = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$CargoToml = Get-Content (Join-Path $RootDir "Cargo.toml")
$VersionLine = $CargoToml | Select-String '^version = "([^"]+)"' | Select-Object -First 1
if (-not $VersionLine) {
    throw "Could not read the BExplorer version from Cargo.toml"
}

$Version = $VersionLine.Matches[0].Groups[1].Value
$DistDir = Join-Path $RootDir "dist"
$StageDir = Join-Path $DistDir "bexplorer-windows-$Target"
$Archive = Join-Path $DistDir "bexplorer-$Version-windows-$Target.zip"
$ArchiveChecksum = "$Archive.sha256.txt"
$Installer = Join-Path $DistDir "BExplorer-$Version-Setup-x64.exe"
$InstallerChecksum = "$Installer.sha256.txt"

function Write-Checksum {
    param([Parameter(Mandatory)][string]$Path)

    $Hash = (Get-FileHash $Path -Algorithm SHA256).Hash.ToLowerInvariant()
    "$Hash  $(Split-Path -Leaf $Path)" | Set-Content "$Path.sha256.txt" -Encoding ascii
    Write-Host "Created $Path.sha256.txt"
}

function Find-InnoSetupCompiler {
    $Command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    if ($Command) {
        return $Command.Source
    }

    $Candidates = @(
        (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 7\ISCC.exe"),
        (Join-Path $env:ProgramFiles "Inno Setup 7\ISCC.exe"),
        (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"),
        (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe")
    ) | Where-Object { $_ }

    return $Candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}

if (-not $SkipInstaller -and $Target -ne "x86_64-pc-windows-msvc") {
    throw "The Inno Setup definition currently packages the x86_64-pc-windows-msvc target. Use -SkipInstaller for another portable target."
}

New-Item $DistDir -ItemType Directory -Force | Out-Null

& cargo build --manifest-path (Join-Path $RootDir "Cargo.toml") --release --target $Target
if ($LASTEXITCODE -ne 0) {
    throw "cargo build failed with exit code $LASTEXITCODE"
}

$Binary = Join-Path $RootDir "target/$Target/release/bexplorer.exe"
if (-not (Test-Path -LiteralPath $Binary)) {
    throw "Release executable was not found at $Binary"
}

Remove-Item $StageDir -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item $Archive -Force -ErrorAction SilentlyContinue
Remove-Item $ArchiveChecksum -Force -ErrorAction SilentlyContinue
Remove-Item $Installer -Force -ErrorAction SilentlyContinue
Remove-Item $InstallerChecksum -Force -ErrorAction SilentlyContinue
New-Item $StageDir -ItemType Directory -Force | Out-Null

Copy-Item $Binary (Join-Path $StageDir "BExplorer.exe")
Copy-Item (Join-Path $RootDir "README.md") $StageDir
Copy-Item (Join-Path $RootDir "LICENSE") $StageDir
Copy-Item (Join-Path $RootDir "THIRD_PARTY_NOTICES.md") $StageDir
Copy-Item (Join-Path $RootDir "vendor/7zip-src/DOC/License.txt") (Join-Path $StageDir "License-7Zip.txt")
Copy-Item (Join-Path $RootDir "vendor/7zip-src/DOC/copying.txt") (Join-Path $StageDir "copying-7Zip.txt")
Copy-Item (Join-Path $RootDir "vendor/7zip-src/DOC/unRarLicense.txt") $StageDir

Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $Archive -CompressionLevel Optimal
Write-Host "Created $Archive"
Write-Checksum $Archive

if ($SkipInstaller) {
    Write-Host "Skipped Inno Setup installer generation"
    exit 0
}

$Iscc = Find-InnoSetupCompiler
if (-not $Iscc) {
    throw "ISCC.exe was not found. Install Inno Setup 6 or 7, or use -SkipInstaller to build only the portable ZIP."
}

$IssPath = Join-Path $PSScriptRoot "BExplorer.iss"
& $Iscc "/DMyAppVersion=$Version" $IssPath
if ($LASTEXITCODE -ne 0) {
    throw "Inno Setup failed with exit code $LASTEXITCODE"
}
if (-not (Test-Path -LiteralPath $Installer)) {
    throw "Inno Setup completed but the installer was not found at $Installer"
}

Write-Host "Created $Installer"
Write-Checksum $Installer

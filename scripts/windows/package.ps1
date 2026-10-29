param(
    [ValidatePattern("^[A-Za-z0-9_]+(?:-[A-Za-z0-9_]+)+$")]
    [string]$Target = "x86_64-pc-windows-msvc",
    [switch]$SkipInstaller
)

$ErrorActionPreference = "Stop"
$RootDir = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$CargoToml = Get-Content -LiteralPath (Join-Path $RootDir "Cargo.toml")
$VersionLine = $CargoToml | Select-String '^version = "([^"]+)"' | Select-Object -First 1
if (-not $VersionLine) {
    throw "Could not read the BExplorer version from Cargo.toml"
}

$Version = $VersionLine.Matches[0].Groups[1].Value
if ($Version -notmatch "^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$") {
    throw "The package version is not a safe SemVer filename component"
}
$DistDir = Join-Path $RootDir "dist"
$StageDir = Join-Path $DistDir "bexplorer-windows-$Target"
$Portable = Join-Path $DistDir "BExplorer-$Version-windows-$Target.exe"
$PortableChecksum = "$Portable.sha256.txt"
$LicenseBundle = Join-Path $DistDir "BExplorer-$Version-licenses.txt"
$Installer = Join-Path $DistDir "BExplorer-$Version-Setup-x64.exe"
$InstallerChecksum = "$Installer.sha256.txt"

function Write-Checksum {
    param([Parameter(Mandatory)][string]$Path)

    $Hash = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
    "$Hash  $(Split-Path -Leaf $Path)" | Set-Content -LiteralPath "$Path.sha256.txt" -Encoding ascii
    Write-Host "Created $Path.sha256.txt"
}

function Find-InnoSetupCompiler {
    $Command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    if ($Command) {
        return $Command.Source
    }

    $Candidates = @(
        (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 7\ISCC.exe"),
        (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe"),
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

# All paths which may be removed are exact, direct children of this repository's
# dist directory. Refuse links before cleanup so a stale junction cannot redirect
# packaging cleanup into another tree.
function Assert-DistChild {
    param([Parameter(Mandatory)][string]$Path)

    $FullPath = [IO.Path]::GetFullPath($Path)
    $FullDist = [IO.Path]::GetFullPath($DistDir).TrimEnd([IO.Path]::DirectorySeparatorChar)
    if ([IO.Path]::GetDirectoryName($FullPath) -ne $FullDist) {
        throw "Refusing packaging cleanup outside dist: $FullPath"
    }
    $Item = Get-Item -LiteralPath $FullPath -Force -ErrorAction SilentlyContinue
    if ($Item -and ($Item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "Refusing a packaging output which is a link: $FullPath"
    }
    return $FullPath
}

function Remove-PackageFile {
    param([Parameter(Mandatory)][string]$Path)

    $FullPath = Assert-DistChild $Path
    $Item = Get-Item -LiteralPath $FullPath -Force -ErrorAction SilentlyContinue
    if ($Item) {
        if ($Item.PSIsContainer) {
            throw "Expected a package file, found a directory: $FullPath"
        }
        Remove-Item -LiteralPath $FullPath -Force
    }
}

function Remove-PackageStage {
    $FullStage = Assert-DistChild $StageDir
    if (-not (Test-Path -LiteralPath $FullStage)) {
        return
    }
    if (-not (Get-Item -LiteralPath $FullStage -Force).PSIsContainer) {
        throw "Expected a packaging staging directory: $FullStage"
    }
    $Pending = [Collections.Generic.Stack[string]]::new()
    $Pending.Push($FullStage)
    while ($Pending.Count -gt 0) {
        $Directory = $Pending.Pop()
        foreach ($Item in Get-ChildItem -LiteralPath $Directory -Force) {
            if ($Item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw "Refusing cleanup of a staging tree containing a link: $($Item.FullName)"
            }
            if ($Item.PSIsContainer) {
                $Pending.Push($Item.FullName)
            }
        }
    }
    Remove-Item -LiteralPath $FullStage -Recurse -Force
}

function Write-LicenseBundle {
    $Utf8 = [Text.UTF8Encoding]::new($false)
    $Stream = [IO.File]::Open($LicenseBundle, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write)
    try {
        foreach ($License in $LicenseFiles) {
            $Header = $Utf8.GetBytes("BExplorer $Version - $($License.Source)`r`n============================================================`r`n")
            $Stream.Write($Header, 0, $Header.Length)
            # Copy the complete source bytes verbatim, preserving every notice.
            $Bytes = [IO.File]::ReadAllBytes((Join-Path $RootDir $License.Source))
            $Stream.Write($Bytes, 0, $Bytes.Length)
            $Separator = $Utf8.GetBytes("`r`n============================================================`r`n`r`n")
            $Stream.Write($Separator, 0, $Separator.Length)
        }
    } finally {
        $Stream.Dispose()
    }
    Write-Host "Created $LicenseBundle"
}

$DistItem = Get-Item -LiteralPath $DistDir -Force -ErrorAction SilentlyContinue
if ($DistItem -and (-not $DistItem.PSIsContainer -or ($DistItem.Attributes -band [IO.FileAttributes]::ReparsePoint))) {
    throw "The dist path must be a normal directory inside the repository"
}
New-Item -Path $DistDir -ItemType Directory -Force | Out-Null

# Run Cargo from the manifest directory so relative CARGO_TARGET_DIR values have
# the same meaning for Cargo and for the executable path below.
Push-Location -LiteralPath $RootDir
try {
    & cargo build --locked --manifest-path (Join-Path $RootDir "Cargo.toml") --release --target $Target
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed with exit code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}

$TargetDir = if ([string]::IsNullOrEmpty($env:CARGO_TARGET_DIR)) { "target" } else { $env:CARGO_TARGET_DIR }
if (-not [IO.Path]::IsPathRooted($TargetDir)) {
    $TargetDir = Join-Path $RootDir $TargetDir
}
$Binary = Join-Path ([IO.Path]::GetFullPath($TargetDir)) "$Target/release/bexplorer.exe"
if (-not (Test-Path -LiteralPath $Binary -PathType Leaf)) {
    throw "Release executable was not found at $Binary"
}

Remove-PackageStage
foreach ($Path in @($Portable, $PortableChecksum, $LicenseBundle, $Installer, $InstallerChecksum)) {
    Remove-PackageFile $Path
}
New-Item -Path $StageDir -ItemType Directory | Out-Null

$LicenseFiles = @(
    @{ Source = "LICENSE"; StagedName = "LICENSE" },
    @{ Source = "THIRD_PARTY_NOTICES.md"; StagedName = "THIRD_PARTY_NOTICES.md" },
    @{ Source = "vendor/7zip-src/DOC/License.txt"; StagedName = "License-7Zip.txt" },
    @{ Source = "vendor/7zip-src/DOC/copying.txt"; StagedName = "copying-7Zip.txt" },
    @{ Source = "vendor/7zip-src/DOC/unRarLicense.txt"; StagedName = "unRarLicense.txt" }
)
Copy-Item -LiteralPath $Binary -Destination (Join-Path $StageDir "BExplorer.exe")
Copy-Item -LiteralPath (Join-Path $RootDir "README.md") -Destination $StageDir
foreach ($License in $LicenseFiles) {
    Copy-Item -LiteralPath (Join-Path $RootDir $License.Source) -Destination (Join-Path $StageDir $License.StagedName)
}
Copy-Item -LiteralPath $Binary -Destination $Portable
Write-Host "Created $Portable"
Write-Checksum $Portable
Write-LicenseBundle

if ($SkipInstaller) {
    Write-Host "Skipped Inno Setup installer generation"
    exit 0
}

$Iscc = Find-InnoSetupCompiler
if (-not $Iscc) {
    throw "ISCC.exe was not found. Install Inno Setup 6 or 7, or use -SkipInstaller to build only the portable executable."
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

param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [switch]$SkipInstaller
)

$ErrorActionPreference = "Stop"
$RootDir = Split-Path -Parent $PSScriptRoot
& (Join-Path $RootDir "scripts/windows/package.ps1") -Target $Target -SkipInstaller:$SkipInstaller
exit $LASTEXITCODE

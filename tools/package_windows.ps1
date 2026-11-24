param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [switch]$SkipInstaller,
    [string]$SigningThumbprint,
    [uri]$TimestampServer
)

$ErrorActionPreference = "Stop"
$RootDir = Split-Path -Parent $PSScriptRoot
& (Join-Path $RootDir "scripts/windows/package.ps1") @PSBoundParameters
exit $LASTEXITCODE

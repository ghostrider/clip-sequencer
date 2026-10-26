<#
  Builds the SeqView FFGL plugin (Release x64) and copies SeqView.dll to Resolume's Extra Effects folder.

  Usage:  powershell -ExecutionPolicy Bypass -File build_seqview.ps1 [-Configuration Release|Debug] [-Target <folder>] [-SkipCopy]

  Safety: an existing SeqView.dll in the target folder is only overwritten if it is ours (contains the plugin ID
  "VJSQ" and the name "SeqView"). Resolume must be closed: it keeps loaded DLLs locked.
#>
param(
    [ValidateSet('Release', 'Debug')] [string] $Configuration = 'Release',
    [string] $Target = (Join-Path $env:USERPROFILE 'Documents\Resolume Arena\Extra Effects'),
    [switch] $SkipCopy
)
$ErrorActionPreference = 'Stop'

$here    = $PSScriptRoot
$project = Join-Path $here 'SeqView.vcxproj'

# MSBuild: not on PATH on this machine. Try vswhere first (also not on PATH by default - resolve it by its own
# well-known install path), then fall back to the known VS install location.
$msbuild = $null
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (Test-Path $vswhere) {
    $msbuild = & $vswhere -latest -products * -requires Microsoft.Component.MSBuild -find 'MSBuild\**\Bin\MSBuild.exe' | Select-Object -First 1
}
if (-not $msbuild) {
    $msbuild = 'C:\Program Files\Microsoft Visual Studio\2022\Community\MSBuild\Current\Bin\MSBuild.exe'
}
if (-not (Test-Path $msbuild)) { throw "MSBuild not found ($msbuild)" }

Write-Host "Building SeqView ($Configuration|x64) ..."
& $msbuild $project "-p:Configuration=$Configuration" '-p:Platform=x64' '-m' '-v:minimal' '-nologo'
if ($LASTEXITCODE -ne 0) { throw "Build failed (exit code $LASTEXITCODE)" }

$dll = Join-Path $here "bin\x64\$Configuration\SeqView.dll"
if (-not (Test-Path $dll)) { throw "Build output missing: $dll" }
Write-Host "Built: $dll"
if ($SkipCopy) { return }

if (-not (Test-Path $Target)) {
    Write-Host "Creating $Target"
    New-Item -ItemType Directory -Force -Path $Target | Out-Null
}

$dest = Join-Path $Target 'SeqView.dll'
if (Test-Path $dest) {
    # Only replace our own plugin, never a different plugin that happens to use the same file name.
    $text = [System.Text.Encoding]::ASCII.GetString([System.IO.File]::ReadAllBytes($dest))
    if (-not ($text.Contains('VJSQ') -and $text.Contains('SeqView'))) {
        throw "$dest exists and is not the SeqView plugin - not overwriting it."
    }
}

try {
    Copy-Item -Force $dll $dest
} catch {
    throw "Copy failed - is Resolume running? It locks loaded plugins. ($($_.Exception.Message))"
}
Write-Host "Installed: $dest  (restart Resolume to load it)"

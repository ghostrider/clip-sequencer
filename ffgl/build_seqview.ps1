<#
  Builds the SeqView FFGL plugin (Release x64, via build.ps1) and copies SeqView.dll to Resolume's Extra Effects
  folder: the developer's build-and-install shortcut. End users install the plugin from the app
  (File > Install FFGL plugin..., or seq-app-cli --install-ffgl); the release uses build.ps1 only.

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

$here = $PSScriptRoot

# Build (ffgl\build.ps1, also used by the release script), then copy.
& (Join-Path $here 'build.ps1') -Configuration $Configuration
$dll = Join-Path $here "bin\x64\$Configuration\SeqView.dll"
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

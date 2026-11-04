<#
  Builds the SeqView FFGL plugin (x64) without installing it: the build step of scripts\release.ps1 and CI
  (M11 Phase G3). build_seqview.ps1 calls it and then copies the DLL into Resolume's plugin folder.

  Usage:  powershell -ExecutionPolicy Bypass -File ffgl\build.ps1 [-Configuration Release|Debug] [-Tests] [-CheckDependencies]
    -Tests              also build tests\SeqViewTests.vcxproj (run tests\bin\<Configuration>\SeqViewTests.exe yourself)
    -CheckDependencies  check with dumpbin that SeqView.dll needs no Visual C++ runtime DLL (static runtime, /MT)

  Output: ffgl\bin\x64\<Configuration>\SeqView.dll (and .pdb). Needs Visual Studio 2022 (or its Build Tools) with
  the C++ desktop workload, and the FFGL SDK submodule (git submodule update --init).
#>
param(
    [ValidateSet('Release', 'Debug')] [string] $Configuration = 'Release',
    [switch] $Tests,
    [switch] $CheckDependencies
)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot

if (-not (Test-Path (Join-Path $here '..\external\ffgl\source\lib\ffgl'))) {
    throw 'FFGL SDK missing: run "git submodule update --init" in the repository.'
}

# vswhere has a fixed install path (it is not on PATH); fall back to the default VS 2022 Community location.
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$msbuild = $null
if (Test-Path $vswhere) {
    $msbuild = & $vswhere -latest -products * -requires Microsoft.Component.MSBuild -find 'MSBuild\**\Bin\MSBuild.exe' | Select-Object -First 1
}
if (-not $msbuild) {
    $msbuild = 'C:\Program Files\Microsoft Visual Studio\2022\Community\MSBuild\Current\Bin\MSBuild.exe'
}
if (-not (Test-Path $msbuild)) { throw "MSBuild not found ($msbuild). Install Visual Studio 2022 with the C++ desktop workload." }

$projects = @(Join-Path $here 'SeqView.vcxproj')
if ($Tests) { $projects += Join-Path $here 'tests\SeqViewTests.vcxproj' }
foreach ($project in $projects) {
    Write-Host "Building $(Split-Path $project -Leaf) ($Configuration|x64) ..."
    & $msbuild $project "-p:Configuration=$Configuration" '-p:Platform=x64' '-m' '-v:minimal' '-nologo'
    if ($LASTEXITCODE -ne 0) { throw "Build failed (exit code $LASTEXITCODE): $project" }
}

$dll = Join-Path $here "bin\x64\$Configuration\SeqView.dll"
if (-not (Test-Path $dll)) { throw "Build output missing: $dll" }
Write-Host "Built: $dll"

if ($CheckDependencies) {
    $dumpbin = $null
    if (Test-Path $vswhere) {
        $dumpbin = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -find 'VC\Tools\MSVC\**\bin\Hostx64\x64\dumpbin.exe' | Select-Object -First 1
    }
    if (-not $dumpbin -or -not (Test-Path $dumpbin)) { throw 'dumpbin.exe not found (needs the MSVC build tools).' }
    # The dependent DLL names are the indented lines ending in .dll (not the "Dump of file ...SeqView.dll" header).
    $deps = & $dumpbin /nologo /dependents $dll | ForEach-Object { $_.Trim() } |
        Where-Object { $_ -match '^[\w.-]+\.dll$' }
    Write-Host "Dependencies of SeqView.dll: $($deps -join ', ')"
    # The static runtime (/MT) means none of these may appear: they would need the VC++ Redistributable.
    $runtime = $deps | Where-Object { $_ -match '^(msvcp|vcruntime|concrt|ucrtbase|api-ms-win-crt)' }
    if ($runtime) { throw "SeqView.dll depends on the Visual C++ runtime: $($runtime -join ', ')" }
    Write-Host 'OK: only system DLLs (static C++ runtime).'
}

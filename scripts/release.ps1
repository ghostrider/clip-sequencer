# Builds the browser UI and the release binaries, copies seq-app.exe (desktop app) and
# seq-app-cli.exe (console, headless) to release\, and packs the portable zip
# release\clip-sequencer-<version>-windows-x64.zip (+ .sha256).
# The binary embeds web\dist, so the UI must be built first. No installer (M12 decision: portable
# only); the exe needs the WebView2 Runtime, which ships with Windows 11.
# Needs cargo-about for the third-party notices: cargo install --locked cargo-about --features cli
# Usage:  powershell -ExecutionPolicy Bypass -File scripts\release.ps1

$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

Write-Host '==> UI (npm ci + build)' -ForegroundColor Cyan
Push-Location web
& npm ci --no-audit --no-fund
if ($LASTEXITCODE -ne 0) { Pop-Location; exit $LASTEXITCODE }
& npm run build
if ($LASTEXITCODE -ne 0) { Pop-Location; exit $LASTEXITCODE }
Pop-Location

# Third-party notices (Rust crates via cargo-about, web/dist/third-party-licenses.json from the UI build,
# fonts, FFGL deps). Before cargo build: the binary embeds THIRD_PARTY_LICENSES.json (License dialog).
Write-Host '==> THIRD_PARTY_LICENSES.md' -ForegroundColor Cyan
& node scripts\third-party-licenses.mjs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# FFGL plugin (Release x64, static C++ runtime; dumpbin checks there is no VC++ runtime dependency).
Write-Host '==> FFGL plugin' -ForegroundColor Cyan
& (Join-Path $PSScriptRoot '..\ffgl\build.ps1') -Configuration Release -CheckDependencies
if (-not $?) { exit 1 }

Write-Host '==> cargo build --release' -ForegroundColor Cyan
& cargo build --release -p seq-app
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

New-Item -ItemType Directory -Force release | Out-Null
Copy-Item target\release\seq-app.exe release\seq-app.exe -Force
Copy-Item target\release\seq-app-cli.exe release\seq-app-cli.exe -Force

# The version the binary reports ("seq-app 0.1.0 (build 42, abc1234, 2026-11-03)"); [workspace.package] is its
# only source. Second word = the semver.
$version = ((& release\seq-app-cli.exe --version) -split ' ')[1]
$name = "clip-sequencer-$version-windows-x64"
Write-Host "==> $name.zip" -ForegroundColor Cyan
$stage = Join-Path release $name
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $stage | Out-Null
Copy-Item release\seq-app.exe, release\seq-app-cli.exe, README.md, LICENSE, THIRD_PARTY_LICENSES.md, OFL.txt $stage
if (Test-Path CHANGELOG.md) { Copy-Item CHANGELOG.md $stage }
# The plugin next to the app: seq-app finds it at <exe folder>\ffgl\SeqView.dll (File > Install FFGL plugin...).
New-Item -ItemType Directory (Join-Path $stage 'ffgl') | Out-Null
Copy-Item ffgl\bin\x64\Release\SeqView.dll (Join-Path $stage 'ffgl')
(Get-Content -Raw ffgl\README.release.txt).Replace('{{VERSION}}', $version) |
    Set-Content -Encoding utf8 (Join-Path $stage 'ffgl\README.txt')
# The .pdb is not shipped; keep it next to the zip for crash reports (release.yml uploads it as an artifact).
Copy-Item ffgl\bin\x64\Release\SeqView.pdb (Join-Path release "SeqView-$version.pdb")
$zip = Join-Path release "$name.zip"
Remove-Item $zip -Force -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
Remove-Item $stage -Recurse -Force
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
"$hash  $name.zip" | Set-Content -Encoding ascii "$zip.sha256"

Write-Host "Done: release\seq-app.exe, release\seq-app-cli.exe, release\$name.zip (sha256 $hash)" -ForegroundColor Green

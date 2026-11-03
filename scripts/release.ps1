# Builds the browser UI and the release binaries, copies seq-app.exe (desktop app) and
# seq-app-cli.exe (console, headless) to release\, and packs the portable zip
# release\clip-sequencer-<version>-windows-x64.zip (+ .sha256).
# The binary embeds web\dist, so the UI must be built first. No installer (M12 decision: portable
# only); the exe needs the WebView2 Runtime, which ships with Windows 11.
# Needs cargo-about for the third-party notices: cargo install --locked cargo-about --features cli
# Usage:  powershell -ExecutionPolicy Bypass -File scripts\release.ps1

$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

Write-Host '==> UI (npm install + build)' -ForegroundColor Cyan
Push-Location web
& npm install --no-audit --no-fund
if ($LASTEXITCODE -ne 0) { Pop-Location; exit $LASTEXITCODE }
& npm run build
if ($LASTEXITCODE -ne 0) { Pop-Location; exit $LASTEXITCODE }
Pop-Location

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
# Third-party notices (Rust crates via cargo-about, web/dist/third-party-licenses.json, fonts, FFGL deps)
Write-Host '==> THIRD_PARTY_LICENSES.md' -ForegroundColor Cyan
& node scripts\third-party-licenses.mjs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Copy-Item release\seq-app.exe, release\seq-app-cli.exe, README.md, LICENSE, THIRD_PARTY_LICENSES.md, OFL.txt $stage
if (Test-Path CHANGELOG.md) { Copy-Item CHANGELOG.md $stage }
$zip = Join-Path release "$name.zip"
Remove-Item $zip -Force -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
Remove-Item $stage -Recurse -Force
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
"$hash  $name.zip" | Set-Content -Encoding ascii "$zip.sha256"

Write-Host "Done: release\seq-app.exe, release\seq-app-cli.exe, release\$name.zip (sha256 $hash)" -ForegroundColor Green

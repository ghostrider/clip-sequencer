# Builds the browser UI and the release binaries, and copies seq-app.exe (desktop app) and
# seq-app-cli.exe (console, headless) to release\.
# The binary embeds web\dist, so the UI must be built first.
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
Write-Host 'Done: release\seq-app.exe, release\seq-app-cli.exe' -ForegroundColor Green

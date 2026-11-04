# CI-style check for the workspace: format, lint, test. Exits non-zero on the first failure.
# Usage (from anywhere):  powershell -ExecutionPolicy Bypass -File scripts\check.ps1 [-Fix]
#   -Fix   run "cargo fmt --all" instead of only checking formatting

param(
    [switch]$Fix
)

$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

function Invoke-Step {
    param([string]$Name, [string[]]$CargoArgs)
    Write-Host "==> $Name" -ForegroundColor Cyan
    & cargo @CargoArgs
    if ($LASTEXITCODE -ne 0) {
        Write-Host "FAILED: $Name" -ForegroundColor Red
        exit $LASTEXITCODE
    }
}

if ($Fix) {
    Invoke-Step 'fmt' @('fmt', '--all')
} else {
    Invoke-Step 'fmt --check' @('fmt', '--all', '--', '--check')
}
Invoke-Step 'clippy' @('clippy', '--workspace', '--all-targets', '--', '-D', 'warnings')
Invoke-Step 'test' @('test', '--workspace')
# One version source: Cargo.toml = web/package.json = package-lock.json (M11 Phase E).
Write-Host '==> version' -ForegroundColor Cyan
& node scripts\check-version.mjs
if ($LASTEXITCODE -ne 0) { Write-Host 'FAILED: version' -ForegroundColor Red; exit $LASTEXITCODE }
# License policy and advisories (deny.toml); CI runs the same check.
if (Get-Command cargo-deny -ErrorAction SilentlyContinue) {
    Invoke-Step 'deny' @('deny', 'check', 'licenses', 'bans', 'advisories', 'sources')
} else {
    Write-Host 'skipped: cargo deny (install: cargo install --locked cargo-deny)' -ForegroundColor Yellow
}

Write-Host 'All checks passed.' -ForegroundColor Green

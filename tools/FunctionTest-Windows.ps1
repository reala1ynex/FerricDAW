# Sequential FerricDAW button/function test.
# Builds (if needed) and runs: ferricdaw.exe --function-test
# Each case invokes one button's handler, checks the result, then moves on.
param(
    # Debug default: release uses panic=abort so catch_unwind cannot isolate failures.
    [switch]$Release,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

if (-not $SkipBuild) {
    Write-Host "Building ferric-app…" -ForegroundColor Cyan
    if ($Release) {
        cargo build --release -p ferric-app
    } else {
        cargo build -p ferric-app
    }
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

$exe = if ($Release) {
    Join-Path $Root 'target\release\ferricdaw.exe'
} else {
    Join-Path $Root 'target\debug\ferricdaw.exe'
}
if (-not (Test-Path $exe)) { throw "Missing $exe" }

# Demo WAV next to exe so "Play demo WAV" can resolve.
$demoSrc = Join-Path $Root 'assets\demo\NightCurrent-Demo.wav'
$demoDst = Join-Path (Split-Path $exe) 'demo\NightCurrent-Demo.wav'
if (Test-Path $demoSrc) {
    New-Item -ItemType Directory -Force -Path (Split-Path $demoDst) | Out-Null
    Copy-Item $demoSrc $demoDst -Force
}

Write-Host "Running function test: $exe --function-test" -ForegroundColor Cyan
& $exe --function-test
$code = $LASTEXITCODE
if ($code -eq 0) {
    Write-Host "Function test PASSED." -ForegroundColor Green
} else {
    Write-Host "Function test FAILED (exit $code)." -ForegroundColor Red
}
exit $code

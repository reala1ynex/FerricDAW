# Build + package FerricDAW portable Windows x64 zip.
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

$CargoToml = Get-Content (Join-Path $Root 'Cargo.toml') -Raw
if ($CargoToml -match 'version\s*=\s*"([^"]+)"') {
    $Version = $Matches[1]
} else {
    throw 'Could not read workspace version from Cargo.toml'
}

Write-Host "Building FerricDAW v$Version release…" -ForegroundColor Cyan
cargo build --release -p ferric-app
if ($LASTEXITCODE -ne 0) { throw 'ferric-app release build failed' }
cargo build --release -p ferric-engine --bin ferric-plugin-bridge
if ($LASTEXITCODE -ne 0) { throw 'ferric-plugin-bridge release build failed' }

# Stage into a unique folder so a running ferricdaw.exe cannot lock the copy target.
$Stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$StageDir = Join-Path $Root "dist\FerricDAW-stage-$Stamp"
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null

function Copy-ReleaseFile([string]$Src, [string]$Dst) {
    $attempt = 0
    while ($true) {
        try {
            Copy-Item $Src $Dst -Force
            return
        } catch {
            $attempt++
            if ($attempt -ge 5) {
                throw "Could not copy '$Src' → '$Dst'. Close FerricDAW (ferricdaw.exe) and retry. $_"
            }
            Start-Sleep -Milliseconds (300 * $attempt)
        }
    }
}

Copy-ReleaseFile (Join-Path $Root 'target\release\ferricdaw.exe') (Join-Path $StageDir 'ferricdaw.exe')
Copy-ReleaseFile (Join-Path $Root 'target\release\ferric-plugin-bridge.exe') (Join-Path $StageDir 'ferric-plugin-bridge.exe')
Copy-Item (Join-Path $Root 'README.md') (Join-Path $StageDir 'README.md') -Force
Copy-Item (Join-Path $Root 'LICENSE-MIT') (Join-Path $StageDir 'LICENSE-MIT') -Force
Copy-Item (Join-Path $Root 'LICENSE-APACHE') (Join-Path $StageDir 'LICENSE-APACHE') -Force
Copy-Item (Join-Path $Root 'tools\SetDs10Capture.ps1') (Join-Path $StageDir 'SetDs10Capture.ps1') -Force

$DemoDir = Join-Path $StageDir 'demo'
New-Item -ItemType Directory -Force -Path $DemoDir | Out-Null
$DemoWav = Join-Path $Root 'assets\demo\NightCurrent-Demo.wav'
if (Test-Path $DemoWav) {
    Copy-Item $DemoWav (Join-Path $DemoDir 'NightCurrent-Demo.wav') -Force
}
$DocsOut = Join-Path $StageDir 'docs'
New-Item -ItemType Directory -Force -Path $DocsOut | Out-Null
foreach ($doc in @('BEGINNER_GUIDE.md', 'NIGHT_CURRENT_DEMO.md')) {
    $src = Join-Path $Root "docs\$doc"
    if (Test-Path $src) { Copy-Item $src (Join-Path $DocsOut $doc) -Force }
}

@"
FerricDAW v$Version — Windows x64 portable

Plugins that crash run in a worker process — not the DAW. Keep ferric-plugin-bridge.exe beside ferricdaw.exe.

1. Double-click ferricdaw.exe
2. First launch: choose Night Current Tutorial / Afterglow playground / Empty
3. Later: 🎓 Tutorial (or Ferric menu → Night Current Tutorial) — Esc closes the coach
4. Finish line: demo\NightCurrent-Demo.wav — Play demo WAV in the coach, or A/B after bounce
5. Written twin: docs\BEGINNER_GUIDE.md
6. Optional DS Line-In: run SetDs10Capture.ps1 (Mic Boost stays OFF)

Shortcuts: Space = Play/Stop · Ctrl+K = Command Palette · F1 = Help · Esc = close coach
"@ | Set-Content -Path (Join-Path $StageDir 'START_HERE.txt') -Encoding UTF8

@"
Night Current — FerricDAW tutorial demo

128 BPM · dark techno · Cm–Ab–Eb–Bb · 16 bars
Layout: intro → groove → hook → stack → bigger hook

This WAV is bounced from the same recipe as the in-app 🎓 Tutorial /
docs\BEGINNER_GUIDE.md (build_tutorial_project).
In Ferric: Tutorial → Play demo WAV to A/B without leaving the app.
"@ | Set-Content -Path (Join-Path $DemoDir 'README.txt') -Encoding UTF8

$ZipName = "FerricDAW-v$Version-win-x64.zip"
$ZipPath = Join-Path $Root "dist\$ZipName"
if (Test-Path $ZipPath) { Remove-Item $ZipPath -Force }
Compress-Archive -Path (Join-Path $StageDir '*') -DestinationPath $ZipPath -Force

# Refresh stable dist\FerricDAW folder for local runs (best-effort if unlocked)
$DistDir = Join-Path $Root 'dist\FerricDAW'
try {
    if (Test-Path $DistDir) { Remove-Item $DistDir -Recurse -Force -ErrorAction Stop }
    Copy-Item $StageDir $DistDir -Recurse -Force
} catch {
    Write-Host "Note: could not refresh dist\FerricDAW (file in use). Zip is still good: $ZipPath" -ForegroundColor Yellow
}

$Desktop = [Environment]::GetFolderPath('Desktop')
$DesktopZip = Join-Path $Desktop $ZipName
Copy-Item $ZipPath $DesktopZip -Force

# Cleanup stage
Remove-Item $StageDir -Recurse -Force -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "Packaged:" -ForegroundColor Green
Write-Host "  $ZipPath"
Write-Host "  $DesktopZip"
Get-Item $ZipPath, $DesktopZip | Format-Table FullName, Length, LastWriteTime -AutoSize

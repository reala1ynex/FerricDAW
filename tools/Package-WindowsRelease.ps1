# Build + package FerricDAW portable Windows x64 zip.
# Optional: .\Package-WindowsRelease.ps1 -GitHubOwner youruser
param(
    [string]$GitHubOwner = $env:FERRIC_GITHUB_OWNER,
    [string]$GitHubRepo = $(if ($env:FERRIC_GITHUB_REPO) { $env:FERRIC_GITHUB_REPO } else { 'FerricDAW' }),
    [switch]$LocalCandidate,
    # Also copy the ZIP to the Desktop (off by default; audit L17).
    [switch]$CopyToDesktop
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
$BuildTarget = if ($env:CARGO_TARGET_DIR) {
    if ([IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) { $env:CARGO_TARGET_DIR }
    else { Join-Path $Root $env:CARGO_TARGET_DIR }
} else { Join-Path $Root 'target' }

$CargoToml = Get-Content (Join-Path $Root 'Cargo.toml') -Raw
if ($CargoToml -match 'version\s*=\s*"([^"]+)"') {
    $Version = $Matches[1]
} else {
    throw 'Could not read workspace version from Cargo.toml'
}

# Record exactly what is being built so publishing can refuse a package that
# does not come from the commit being released (audit M13).
$BuildCommit = (git rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $BuildCommit -notmatch '^[0-9a-f]{40}$') { throw 'Could not resolve the build commit.' }
$BuildBranch = (git rev-parse --abbrev-ref HEAD).Trim()
$BuildDirty = [bool](git status --porcelain --untracked-files=no)

Write-Host "Building FerricDAW v$Version release from $BuildBranch@$($BuildCommit.Substring(0, 12))$(if ($BuildDirty) { ' (uncommitted changes!)' })…" -ForegroundColor Cyan
cargo build --release -p ferric-app
if ($LASTEXITCODE -ne 0) { throw 'ferric-app release build failed' }
cargo build --release -p ferric-engine --bin ferric-plugin-bridge
if ($LASTEXITCODE -ne 0) { throw 'ferric-plugin-bridge release build failed' }

# Stage into a unique folder so a running ferricdaw.exe cannot lock the copy target.
$Stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$OutputDir = if ($LocalCandidate) {
    Join-Path $Root 'dist\candidate'
} else {
    Join-Path $Root 'dist'
}
$StageDir = Join-Path $OutputDir "FerricDAW-stage-$Stamp"
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

Copy-ReleaseFile (Join-Path $BuildTarget 'release\ferricdaw.exe') (Join-Path $StageDir 'ferricdaw.exe')
Copy-ReleaseFile (Join-Path $BuildTarget 'release\ferric-plugin-bridge.exe') (Join-Path $StageDir 'ferric-plugin-bridge.exe')
Copy-Item (Join-Path $Root 'README.md') (Join-Path $StageDir 'README.md') -Force
Copy-Item (Join-Path $Root 'LICENSE-MIT') (Join-Path $StageDir 'LICENSE-MIT') -Force
Copy-Item (Join-Path $Root 'LICENSE-APACHE') (Join-Path $StageDir 'LICENSE-APACHE') -Force
Copy-Item (Join-Path $Root 'THIRD-PARTY-NOTICES.txt') (Join-Path $StageDir 'THIRD-PARTY-NOTICES.txt') -Force
if (Test-Path (Join-Path $Root 'AUTHORS.md')) {
    Copy-Item (Join-Path $Root 'AUTHORS.md') (Join-Path $StageDir 'AUTHORS.md') -Force
}
Copy-Item (Join-Path $Root 'tools\SetLineInCapture.ps1') (Join-Path $StageDir 'SetLineInCapture.ps1') -Force
Copy-Item (Join-Path $Root 'tools\SetDs10Capture.ps1') (Join-Path $StageDir 'SetDs10Capture.ps1') -Force

$DemoDir = Join-Path $StageDir 'demo'
New-Item -ItemType Directory -Force -Path $DemoDir | Out-Null
$DemoWav = Join-Path $Root 'assets\demo\NightCurrent-Demo.wav'
if (Test-Path $DemoWav) {
    Copy-Item $DemoWav (Join-Path $DemoDir 'NightCurrent-Demo.wav') -Force
}
$DocsOut = Join-Path $StageDir 'docs'
New-Item -ItemType Directory -Force -Path $DocsOut | Out-Null
foreach ($doc in @('BEGINNER_GUIDE.md', 'FEATURE_GUIDE.md', 'NIGHT_CURRENT_DEMO.md', 'DEVLOG.md')) {
    $src = Join-Path $Root "docs\$doc"
    if (Test-Path $src) { Copy-Item $src (Join-Path $DocsOut $doc) -Force }
}

$utf8NoBom = [System.Text.UTF8Encoding]::new($false)
$startHere = @"
FerricDAW v$Version — Windows portable

Keep ferric-plugin-bridge.exe next to ferricdaw.exe. Crashing plugins die in that worker, not the DAW.
Keep these files in their own folder (for example %LOCALAPPDATA%\Programs\FerricDAW), not loose on the Desktop.
If FerricDAW starts from the Desktop or Downloads it offers to move itself there and add a desktop shortcut.

1. Double-click ferricdaw.exe
2. WAV, FLAC, MP3, and Ogg Vorbis export are built in.
3. FerricDAW opens an empty session. Choose Night Current tutorial or Afterglow playground from the Ferric menu.
4. Reopen the tutorial anytime: 🎓 Tutorial (Ferric menu / Ctrl+K). Esc closes it.
5. Target bounce: demo\NightCurrent-Demo.wav
6. Docs: docs\BEGINNER_GUIDE.md · docs\FEATURE_GUIDE.md · docs\DEVLOG.md · AUTHORS.md · F1 in the app
7. MIDI + Line-In: Browser → Hardware Rig. Optional helper: SetLineInCapture.ps1 (Mic Boost off)
8. Owner: Patrick Rudolph (github.com/reala1ynex) · Issues on the GitHub repo

Space = play/stop · Ctrl+K = palette · F1 = help · Ferric → Check for Updates
"@
[System.IO.File]::WriteAllText((Join-Path $StageDir 'START_HERE.txt'), $startHere, $utf8NoBom)

$demoReadme = @"
Night Current — tutorial demo

128 BPM · dark techno · Cm–Ab–Eb–Bb · 16 bars
Intro → groove → hook → stack → bigger hook

Same recipe as the in-app tutorial and docs\BEGINNER_GUIDE.md.
In Ferric: Tutorial → Play demo WAV to compare without leaving the app.
"@
[System.IO.File]::WriteAllText((Join-Path $DemoDir 'README.txt'), $demoReadme, $utf8NoBom)

if ($GitHubOwner) {
    $FeedUrl = "https://github.com/$GitHubOwner/$GitHubRepo/releases/latest/download/latest.json"
    [System.IO.File]::WriteAllText(
        (Join-Path $StageDir 'update_feed.url'),
        $FeedUrl,
        [System.Text.UTF8Encoding]::new($false)
    )
    Write-Host "Update feed: $FeedUrl" -ForegroundColor Cyan
}

# Release manifest: lets FerricDAW identify its own program files by path,
# size and SHA-256 when moving out of, or cleaning, a shared folder.
$ManifestFiles = Get-ChildItem -LiteralPath $StageDir -Recurse -File | Sort-Object FullName | ForEach-Object {
    [ordered]@{
        path   = $_.FullName.Substring($StageDir.Length + 1).Replace('\', '/')
        size   = [int64]$_.Length
        sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}
$Manifest = [ordered]@{ version = $Version; files = @($ManifestFiles) }
[System.IO.File]::WriteAllText(
    (Join-Path $StageDir 'ferricdaw-manifest.json'),
    ($Manifest | ConvertTo-Json -Depth 4),
    [System.Text.UTF8Encoding]::new($false)
)

$ZipName = "FerricDAW-v$Version-win-x64.zip"
$ZipPath = Join-Path $OutputDir $ZipName
# Never fail because Explorer / a running copy has the old zip open — write then swap.
$ZipTmp = Join-Path $OutputDir "$ZipName.$Stamp.tmp.zip"
if (Test-Path $ZipTmp) { Remove-Item $ZipTmp -Force -ErrorAction SilentlyContinue }
Compress-Archive -Path (Join-Path $StageDir '*') -DestinationPath $ZipTmp -Force
try {
    if (Test-Path $ZipPath) { Remove-Item $ZipPath -Force -ErrorAction Stop }
    Move-Item $ZipTmp $ZipPath -Force
} catch {
    # Keep the temp name as the deliverable if the stable path is locked.
    $ZipPath = $ZipTmp
    Write-Host "Note: could not replace $ZipName (file in use). Using: $ZipPath" -ForegroundColor Yellow
}

# latest.json for GitHub Releases auto-update (sha256 of the zip)
$Hash = (Get-FileHash -Algorithm SHA256 -Path $ZipPath).Hash.ToLowerInvariant()
$DownloadUrl = if ($GitHubOwner) {
    "https://github.com/$GitHubOwner/$GitHubRepo/releases/download/v$Version/$ZipName"
} else {
    "file:///$($ZipPath -replace '\\','/')"
}
$Latest = [ordered]@{
    version = $Version
    url     = $DownloadUrl
    sha256  = $Hash
    notes   = "FerricDAW v$Version portable Windows x64"
}
$LatestPath = Join-Path $OutputDir 'latest.json'
# UTF-8 *without* BOM — PowerShell's utf8 encoding adds EF BB BF and breaks serde/ureq JSON parse.
$json = ($Latest | ConvertTo-Json -Compress)
[System.IO.File]::WriteAllText($LatestPath, $json, [System.Text.UTF8Encoding]::new($false))

$BuildInfo = [ordered]@{
    version    = $Version
    commit     = $BuildCommit
    branch     = $BuildBranch
    dirty      = $BuildDirty
    zip_name   = (Split-Path -Leaf $ZipPath)
    zip_sha256 = $Hash
    built_at   = (Get-Date).ToUniversalTime().ToString('o')
}
$BuildInfoPath = Join-Path $OutputDir 'build-info.json'
[System.IO.File]::WriteAllText($BuildInfoPath, ($BuildInfo | ConvertTo-Json), [System.Text.UTF8Encoding]::new($false))

if ($LocalCandidate) {
    # Retain the exact staged executable for the final Windows run. The
    # candidate neither overwrites the shipped zip nor writes to Desktop.
    Write-Host "Candidate zip: $ZipPath"
    Write-Host "Candidate feed: $LatestPath"
    Write-Host "Build info: $BuildInfoPath"
    Write-Host "Candidate executable: $(Join-Path $StageDir 'ferricdaw.exe')"
    return
}

# Optional local folder refresh — skip entirely if anything inside is locked (don't delete).
$DistDir = Join-Path $Root 'dist\FerricDAW'
$refreshed = $false
try {
    $lockProbe = Join-Path $DistDir 'ferricdaw.exe'
    if (Test-Path $lockProbe) {
        # Open for write share — fails if running/locked
        $fs = [System.IO.File]::Open($lockProbe, 'Open', 'Write', 'None')
        $fs.Close()
    }
    if (-not (Test-Path $DistDir)) {
        New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
    }
    Copy-Item (Join-Path $StageDir '*') $DistDir -Recurse -Force
    $refreshed = $true
} catch {
    Write-Host "Note: left dist\FerricDAW alone (file in use / app open). Zip is still good." -ForegroundColor Yellow
}

$DesktopZip = $null
if ($CopyToDesktop) {
    $Desktop = [Environment]::GetFolderPath('Desktop')
    $DesktopZip = Join-Path $Desktop $ZipName
    try {
        Copy-Item $ZipPath $DesktopZip -Force -ErrorAction Stop
    } catch {
        $DesktopZip = $null
        Write-Host "Note: could not copy zip to Desktop (file in use)." -ForegroundColor Yellow
    }
}

# Cleanup stage (best-effort)
Remove-Item $StageDir -Recurse -Force -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "Packaged:" -ForegroundColor Green
Write-Host "  $ZipPath"
if ($DesktopZip) { Write-Host "  $DesktopZip" }
Write-Host "  $LatestPath"
if ($refreshed) { Write-Host "  $DistDir (refreshed)" }
$show = @($ZipPath, $LatestPath)
if ($DesktopZip) { $show += $DesktopZip }
Get-Item $show | Format-Table FullName, Length, LastWriteTime -AutoSize

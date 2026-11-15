# Package FerricDAW and publish a GitHub Release with zip + latest.json (auto-update feed).
# Prerequisites: gh auth login · git remote origin → github.com/<you>/FerricDAW
param(
    [string]$GitHubOwner = $env:FERRIC_GITHUB_OWNER,
    [string]$GitHubRepo = $(if ($env:FERRIC_GITHUB_REPO) { $env:FERRIC_GITHUB_REPO } else { 'FerricDAW' }),
    [switch]$Draft
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

if (-not $GitHubOwner) {
    try {
        $GitHubOwner = (gh api user -q .login).Trim()
    } catch {
        throw "Set -GitHubOwner or run 'gh auth login' first."
    }
}

$env:FERRIC_GITHUB_OWNER = $GitHubOwner
$env:FERRIC_GITHUB_REPO = $GitHubRepo

& (Join-Path $PSScriptRoot 'Package-WindowsRelease.ps1') -GitHubOwner $GitHubOwner -GitHubRepo $GitHubRepo

$CargoToml = Get-Content (Join-Path $Root 'Cargo.toml') -Raw
if ($CargoToml -match 'version\s*=\s*"([^"]+)"') {
    $Version = $Matches[1]
} else {
    throw 'Could not read version from Cargo.toml'
}

$Tag = "v$Version"
$ZipName = "FerricDAW-v$Version-win-x64.zip"
$ZipPath = Join-Path $Root "dist\$ZipName"
$LatestPath = Join-Path $Root 'dist\latest.json'
if (-not (Test-Path $ZipPath)) { throw "Missing $ZipPath" }
if (-not (Test-Path $LatestPath)) { throw "Missing $LatestPath" }

# Ensure tag exists locally (annotated)
$existing = git tag -l $Tag
if (-not $existing) {
    git tag -a $Tag -m "FerricDAW $Tag"
}

Write-Host "Pushing tag $Tag…" -ForegroundColor Cyan
git push origin $Tag 2>$null
git push origin HEAD 2>$null

$notes = @"
## FerricDAW $Tag

Portable Windows x64 build.

1. Unzip ``$ZipName``
2. Run ``ferricdaw.exe`` (keep ``ferric-plugin-bridge.exe`` beside it)
3. In-app updates: **Ferric → Check for Updates…**

``latest.json`` is the auto-update feed for this release channel.
"@

$ghArgs = @(
    'release', 'create', $Tag,
    $ZipPath,
    $LatestPath,
    '--title', "FerricDAW $Tag",
    '--notes', $notes
)
if ($Draft) { $ghArgs += '--draft' }
gh release view $Tag 2>$null | Out-Null
if ($LASTEXITCODE -eq 0) {
    Write-Host "Release $Tag already exists — uploading assets…" -ForegroundColor Yellow
    gh release upload $Tag $ZipPath $LatestPath --clobber
} else {
    & gh @ghArgs
}

Write-Host ""
Write-Host "Published:" -ForegroundColor Green
Write-Host "  https://github.com/$GitHubOwner/$GitHubRepo/releases/tag/$Tag"
Write-Host "  Feed: https://github.com/$GitHubOwner/$GitHubRepo/releases/latest/download/latest.json"

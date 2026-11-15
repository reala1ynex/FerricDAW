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

# Stale env tokens break gh; prefer keyring login.
Remove-Item Env:GH_TOKEN -ErrorAction SilentlyContinue
Remove-Item Env:GITHUB_TOKEN -ErrorAction SilentlyContinue

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
if ($LASTEXITCODE -ne 0 -and $null -ne $LASTEXITCODE) {
    throw "Package-WindowsRelease.ps1 failed (exit $LASTEXITCODE)."
}

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
if (-not (Test-Path $ZipPath)) {
    # Lock-tolerant package may have left a stamped tmp zip — pick newest match.
    $alt = Get-ChildItem (Join-Path $Root 'dist') -Filter "FerricDAW-v$Version-win-x64.zip*" |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
    if ($alt) { $ZipPath = $alt.FullName } else { throw "Missing dist\$ZipName" }
}
if (-not (Test-Path $LatestPath)) { throw "Missing $LatestPath" }

# Ensure tag exists locally (annotated)
$existing = git tag -l $Tag
if (-not $existing) {
    git tag -a $Tag -m "FerricDAW $Tag"
}

Write-Host "Pushing tag $Tag…" -ForegroundColor Cyan
# git writes progress to stderr; don't let PowerShell treat that as failure
cmd /c "git push origin $Tag"
if ($LASTEXITCODE -ne 0) { throw "git push origin $Tag failed" }
cmd /c "git push origin HEAD"
if ($LASTEXITCODE -ne 0) { throw "git push origin HEAD failed" }

$notes = @"
## FerricDAW $Tag

Portable Windows x64 build.

1. Unzip ``$ZipName``
2. Run ``ferricdaw.exe`` (keep ``ferric-plugin-bridge.exe`` beside it)
3. In-app updates: **Ferric → Check for Updates…**

``latest.json`` is the auto-update feed for this release channel.
"@

$releaseExists = $false
try {
    gh release view $Tag 2>$null | Out-Null
    if ($LASTEXITCODE -eq 0) { $releaseExists = $true }
} catch {
    $releaseExists = $false
}

if ($releaseExists) {
    Write-Host "Release $Tag already exists — uploading assets…" -ForegroundColor Yellow
    gh release upload $Tag $ZipPath $LatestPath --clobber
} else {
    $ghArgs = @(
        'release', 'create', $Tag,
        $ZipPath,
        $LatestPath,
        '--title', "FerricDAW $Tag",
        '--notes', $notes
    )
    if ($Draft) { $ghArgs += '--draft' }
    & gh @ghArgs
}

Write-Host ""
Write-Host "Published:" -ForegroundColor Green
Write-Host "  https://github.com/$GitHubOwner/$GitHubRepo/releases/tag/$Tag"
Write-Host "  Feed: https://github.com/$GitHubOwner/$GitHubRepo/releases/latest/download/latest.json"

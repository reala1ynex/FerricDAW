# Package FerricDAW and publish a GitHub Release with zip + latest.json (auto-update feed).
# Prerequisites: gh auth login · git remote origin → github.com/<you>/FerricDAW
param(
    [string]$GitHubOwner = $env:FERRIC_GITHUB_OWNER,
    [string]$GitHubRepo = $(if ($env:FERRIC_GITHUB_REPO) { $env:FERRIC_GITHUB_REPO } else { 'FerricDAW' }),
    [switch]$Draft,
    [string]$VerifiedPackageDir,
    [string]$NotesFile
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

$Repository = "$GitHubOwner/$GitHubRepo"
$Origin = (git remote get-url origin).Trim()
if ($LASTEXITCODE -ne 0 -or $Origin.TrimEnd('/') -notin @(
    "https://github.com/$Repository.git", "https://github.com/$Repository", "git@github.com:$Repository.git"
)) {
    throw "Git origin does not match $Repository."
}

if ($VerifiedPackageDir) {
    $PackageDir = (Resolve-Path -LiteralPath $VerifiedPackageDir).Path
} else {
    & (Join-Path $PSScriptRoot 'Package-WindowsRelease.ps1') -LocalCandidate -GitHubOwner $GitHubOwner -GitHubRepo $GitHubRepo
    $PackageDir = Join-Path $Root 'dist/candidate'
}

$CargoToml = Get-Content (Join-Path $Root 'Cargo.toml') -Raw
if ($CargoToml -match 'version\s*=\s*"([^"]+)"') {
    $Version = $Matches[1]
} else {
    throw 'Could not read version from Cargo.toml'
}

$Tag = "v$Version"
$ZipName = "FerricDAW-v$Version-win-x64.zip"
$ZipPath = Join-Path $PackageDir $ZipName
$LatestPath = Join-Path $PackageDir 'latest.json'
if (-not (Test-Path -LiteralPath $ZipPath -PathType Leaf)) { throw "Missing $ZipPath" }
if (-not (Test-Path -LiteralPath $LatestPath -PathType Leaf)) { throw "Missing $LatestPath" }

# Guard: refuse to publish a BOM-poisoned feed (breaks Ferric → Check for Updates).
$feedBytes = [IO.File]::ReadAllBytes($LatestPath)
if ($feedBytes.Length -ge 3 -and $feedBytes[0] -eq 0xEF -and $feedBytes[1] -eq 0xBB -and $feedBytes[2] -eq 0xBF) {
    throw "dist\latest.json has a UTF-8 BOM — rewrite with Package-WindowsRelease.ps1 before publishing."
}
if ($feedBytes.Length -lt 1 -or [char]$feedBytes[0] -ne '{') {
    throw "dist\latest.json does not look like JSON (must start with '{')."
}

$Feed = Get-Content -LiteralPath $LatestPath -Raw | ConvertFrom-Json
$ZipHash = (Get-FileHash -LiteralPath $ZipPath -Algorithm SHA256).Hash
$ExpectedUrl = "https://github.com/$Repository/releases/download/$Tag/$ZipName"
if ($Feed.version -ne $Version -or $Feed.url -cne $ExpectedUrl -or $Feed.sha256 -ne $ZipHash) {
    throw 'The update feed must match the version, URL, and SHA-256 of the package.'
}

$Commit = (git rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0) { throw 'Could not resolve the release commit.' }
$existing = git tag -l $Tag
if (-not $existing) {
    git tag -a $Tag $Commit -m "FerricDAW $Tag"
    if ($LASTEXITCODE -ne 0) { throw "Could not create $Tag." }
} elseif ((git rev-list -n 1 $Tag).Trim() -ne $Commit) {
    throw "$Tag already points at a different commit."
}

Write-Host "Pushing $Tag and HEAD..." -ForegroundColor Cyan
git push --atomic origin HEAD "refs/tags/$Tag"
if ($LASTEXITCODE -ne 0) { throw "Could not push HEAD and $Tag." }

$notes = @"
## FerricDAW $Tag

Portable Windows x64 build.

1. Unzip ``$ZipName``
2. Run ``ferricdaw.exe`` (keep ``ferric-plugin-bridge.exe`` beside it)
3. In-app updates: **Ferric → Check for Updates…**

``latest.json`` is the auto-update feed for this release channel.
"@

$ghArgs = @(
    'release', 'create', $Tag, $ZipPath, $LatestPath,
    '--repo', $Repository, '--verify-tag', '--title', "FerricDAW $Tag"
)
if ($NotesFile) {
    $ghArgs += @('--notes-file', (Resolve-Path -LiteralPath $NotesFile).Path)
} else {
    $ghArgs += @('--notes', $notes)
}
if ($Draft) { $ghArgs += '--draft' }
& gh @ghArgs
if ($LASTEXITCODE -ne 0) { throw "gh release create failed for $Tag" }

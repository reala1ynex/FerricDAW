# Package FerricDAW and publish a GitHub Release with zip + latest.json (auto-update feed).
# Prerequisites: gh auth login · git remote origin → github.com/<you>/FerricDAW
param(
    [string]$GitHubOwner = $env:FERRIC_GITHUB_OWNER,
    [string]$GitHubRepo = $(if ($env:FERRIC_GITHUB_REPO) { $env:FERRIC_GITHUB_REPO } else { 'FerricDAW' }),
    [switch]$Draft,
    [string]$VerifiedPackageDir,
    [string]$NotesFile,
    # Releases are cut from this branch only (audit M13).
    [string]$ReleaseBranch = 'master'
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

# Refuse to publish from a dirty tree or the wrong branch (audit M13).
$Dirty = git status --porcelain
if ($LASTEXITCODE -ne 0) { throw 'git status failed.' }
if ($Dirty) { throw "The working tree has uncommitted changes. Commit or stash them before publishing.`n$($Dirty -join "`n")" }
$Branch = (git rev-parse --abbrev-ref HEAD).Trim()
if ($Branch -ne $ReleaseBranch) { throw "Releases are published from '$ReleaseBranch', but HEAD is on '$Branch'." }
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

# The package must have been built from exactly this commit, clean.
$BuildInfoPath = Join-Path $PackageDir 'build-info.json'
if (-not (Test-Path -LiteralPath $BuildInfoPath -PathType Leaf)) {
    throw "Missing $BuildInfoPath. Rebuild the package with Package-WindowsRelease.ps1."
}
$BuildInfo = Get-Content -LiteralPath $BuildInfoPath -Raw | ConvertFrom-Json
if ($BuildInfo.commit -ne $Commit) {
    throw "The package was built from $($BuildInfo.commit), but HEAD is $Commit. Rebuild before publishing."
}
if ($BuildInfo.dirty) { throw 'The package was built from a tree with uncommitted changes. Rebuild from a clean tree.' }
if ($BuildInfo.version -ne $Version) { throw "The package is v$($BuildInfo.version), Cargo.toml says v$Version." }
if ($BuildInfo.zip_sha256 -ne $ZipHash) { throw 'The ZIP on disk does not match the SHA-256 recorded at build time.' }
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

**Already have FerricDAW?** Use **Ferric → Check for Updates…** in the app.

**New install**
1. Unzip ``$ZipName`` into its own folder, for example ``%LOCALAPPDATA%\Programs\FerricDAW``. Don't unzip it loose onto the Desktop or into Downloads.
2. Run ``ferricdaw.exe`` (keep ``ferric-plugin-bridge.exe`` beside it).
3. If FerricDAW starts from the Desktop or Downloads, it offers to move itself into its own folder and add a desktop shortcut.

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

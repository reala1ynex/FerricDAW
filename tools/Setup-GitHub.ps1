# One-shot: authenticate (if needed), create public FerricDAW repo, set origin, push master.
param(
    [string]$RepoName = 'FerricDAW',
    [ValidateSet('public', 'private')]
    [string]$Visibility = 'public'
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

Remove-Item Env:GH_TOKEN -ErrorAction SilentlyContinue
Remove-Item Env:GITHUB_TOKEN -ErrorAction SilentlyContinue

gh auth status 2>$null
if ($LASTEXITCODE -ne 0) {
    Write-Host "Opening GitHub login in your browser…" -ForegroundColor Cyan
    gh auth login --hostname github.com --git-protocol https --web
}

$User = (gh api user -q .login).Trim()
if (-not $User) { throw 'Could not read GitHub username — run gh auth login' }

Write-Host "GitHub user: $User" -ForegroundColor Green
$Full = "$User/$RepoName"

# Create repo if missing
$repoExists = $false
try {
    gh repo view $Full 2>$null | Out-Null
    if ($LASTEXITCODE -eq 0) { $repoExists = $true }
} catch {
    $repoExists = $false
}

if (-not $repoExists) {
    Write-Host "Creating $Visibility repo $Full…" -ForegroundColor Cyan
    gh repo create $RepoName --$Visibility --source=. --remote=origin --description "FerricDAW — pure-Rust hybrid DAW"
} else {
    Write-Host "Repo $Full already exists." -ForegroundColor Yellow
    $origin = git remote get-url origin 2>$null
    if (-not $origin) {
        git remote add origin "https://github.com/$Full.git"
    } else {
        git remote set-url origin "https://github.com/$Full.git"
    }
}

$env:FERRIC_GITHUB_OWNER = $User
$env:FERRIC_GITHUB_REPO = $RepoName
[Environment]::SetEnvironmentVariable('FERRIC_GITHUB_OWNER', $User, 'User')
[Environment]::SetEnvironmentVariable('FERRIC_GITHUB_REPO', $RepoName, 'User')

Write-Host "Pushing master…" -ForegroundColor Cyan
# git progress goes to stderr; PowerShell Stop would abort a successful push
cmd /c "git push -u origin master"
if ($LASTEXITCODE -ne 0) { throw "git push -u origin master failed" }

Write-Host ""
Write-Host "Done." -ForegroundColor Green
Write-Host "  Repo: https://github.com/$Full"
Write-Host "  Next: commit your work, then run:"
Write-Host "    powershell -ExecutionPolicy Bypass -File tools\Publish-GitHubRelease.ps1"
Write-Host "  Or tag a release: git tag v1.1.3 && git push origin v1.1.3  (CI packages + uploads)"
# Regenerate crates/ferric-app/src/known_packages.json from released portable ZIPs.
# The app only ever removes stray files whose relative path, size and SHA-256
# match an entry here (or the ferricdaw-manifest.json shipped beside the exe).
param(
    [Parameter(Mandatory = $true)][string[]]$Zip,
    [string]$Out = (Join-Path (Split-Path -Parent $PSScriptRoot) 'crates\ferric-app\src\known_packages.json')
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$sha = [Security.Cryptography.SHA256]::Create()
$packages = [Collections.Generic.List[object]]::new()
foreach ($path in $Zip) {
    $full = (Resolve-Path -LiteralPath $path).Path
    if ((Split-Path -Leaf $full) -notmatch 'v(\d+\.\d+\.\d+)') { throw "No version in $full" }
    $version = $Matches[1]
    $zipHash = (Get-FileHash -LiteralPath $full -Algorithm SHA256).Hash.ToLowerInvariant()
    $archive = [IO.Compression.ZipFile]::OpenRead($full)
    try {
        $files = foreach ($entry in $archive.Entries) {
            if ($entry.FullName.EndsWith('/') -or $entry.FullName.EndsWith('\')) { continue }
            $stream = $entry.Open()
            try { $hash = -join ($sha.ComputeHash($stream) | ForEach-Object { $_.ToString('x2') }) } finally { $stream.Dispose() }
            [ordered]@{ path = ($entry.FullName -replace '\\', '/'); size = [int64]$entry.Length; sha256 = $hash }
        }
    } finally { $archive.Dispose() }
    $packages.Add([ordered]@{ version = $version; zip_sha256 = $zipHash; files = @($files | Sort-Object { $_.path }) })
}
$doc = [ordered]@{ packages = @($packages | Sort-Object { [version]$_.version }, { $_.zip_sha256 }) }
[IO.File]::WriteAllText($Out, ($doc | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))
Write-Host "Wrote $Out with $($packages.Count) packages"
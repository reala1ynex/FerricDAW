param(
    [Parameter(Mandatory = $true)][string]$Zip,
    [Parameter(Mandatory = $true)][string]$Destination,
    [Parameter(Mandatory = $true)][string]$ExeName,
    [Parameter(Mandatory = $true)][int]$AppPid,
    [Parameter(Mandatory = $true)][string]$Sha256,
    # Set when the running copy sat in a shared folder (Desktop, Downloads):
    # the release goes to the dedicated folder and the new version removes the
    # old copy after it starts.
    [string]$RelocatedFrom = ''
)

$ErrorActionPreference = 'Stop'
$Temp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$Stage = Join-Path $Temp ('ferricdaw-update-stage-' + [guid]::NewGuid().ToString('N'))
$Extracted = Join-Path $Stage 'files'
$Backup = Join-Path $Stage 'previous'
$Log = [IO.Path]::ChangeExtension($PSCommandPath, '.log')
$Touched = [Collections.Generic.List[object]]::new()
$KeepBackup = $false
$Success = $false
try {
    $App = Get-Process -Id $AppPid -ErrorAction SilentlyContinue
    if ($App -and -not $App.WaitForExit(60000)) {
        throw 'FerricDAW did not close. No installation files were changed.'
    }
    if ((Get-FileHash -LiteralPath $Zip -Algorithm SHA256).Hash -ne $Sha256) {
        throw 'The downloaded ZIP no longer matches the verified release checksum.'
    }
    Expand-Archive -LiteralPath $Zip -DestinationPath $Extracted
    $Kids = @(Get-ChildItem -LiteralPath $Extracted)
    $Source = if ($Kids.Count -eq 1 -and $Kids[0].PSIsContainer) { $Kids[0].FullName } else { $Extracted }
    foreach ($Name in @($ExeName, 'ferric-plugin-bridge.exe')) {
        if (-not (Test-Path -LiteralPath (Join-Path $Source $Name) -PathType Leaf)) {
            throw "The update archive is missing $Name."
        }
    }
    foreach ($File in Get-ChildItem -LiteralPath $Source -Recurse -File) {
        $Relative = $File.FullName.Substring($Source.Length + 1)
        $Target = Join-Path $Destination $Relative
        $Previous = Join-Path $Backup $Relative
        New-Item -ItemType Directory -Path (Split-Path -Parent $Target) -Force | Out-Null
        if (Test-Path -LiteralPath $Target -PathType Container) {
            throw "A folder occupies the release file path $Target."
        }
        if (Test-Path -LiteralPath $Target -PathType Leaf) {
            New-Item -ItemType Directory -Path (Split-Path -Parent $Previous) -Force | Out-Null
            Move-Item -LiteralPath $Target -Destination $Previous
        }
        $Touched.Add(@{ Target = $Target; Previous = $Previous })
        Copy-Item -LiteralPath $File.FullName -Destination $Target
    }
    $LaunchArgs = @{ FilePath = (Join-Path $Destination $ExeName); WorkingDirectory = $Destination; WindowStyle = 'Normal' }
    if ($RelocatedFrom) {
        $LaunchArgs.ArgumentList = @('--relocated-from', ('"' + $RelocatedFrom.TrimEnd('\') + '"'))
    }
    Start-Process @LaunchArgs
    $Success = $true
} catch {
    $Failure = $_
    for ($Index = $Touched.Count - 1; $Index -ge 0; $Index--) {
        $File = $Touched[$Index]
        try {
            if (Test-Path -LiteralPath $File.Target -PathType Leaf) {
                Remove-Item -LiteralPath $File.Target -Force
            }
            if (Test-Path -LiteralPath $File.Previous -PathType Leaf) {
                Move-Item -LiteralPath $File.Previous -Destination $File.Target
            }
        } catch {
            $KeepBackup = $true
            Add-Content -LiteralPath $Log -Value "Could not restore $($File.Target) from $($File.Previous): $_"
        }
    }
    Add-Content -LiteralPath $Log -Value $Failure
    throw $Failure
} finally {
    if ((Test-Path -LiteralPath $Stage) -and -not $KeepBackup) {
        $ResolvedStage = (Resolve-Path -LiteralPath $Stage).Path
        if ([IO.Path]::GetDirectoryName($ResolvedStage) -eq $Temp.TrimEnd('\')) {
            Remove-Item -LiteralPath $ResolvedStage -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
    if ($Success) {
        Remove-Item -LiteralPath $Zip, $PSCommandPath -Force -ErrorAction SilentlyContinue
    }
}

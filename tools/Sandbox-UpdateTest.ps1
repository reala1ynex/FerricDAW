# Sandbox reproduction of the Desktop install bug and the v1.1.14+ repair.
# Never touches the real Desktop, Downloads, Start Menu, TEMP or LOCALAPPDATA:
# FerricDAW's FERRIC_*_DIR overrides and LOCALAPPDATA/TEMP point into $Root.
param(
    [Parameter(Mandatory = $true)][string]$Root,
    [Parameter(Mandatory = $true)][ValidateSet('from-1.1.12', 'from-1.1.13', 'from-1.1.14', 'new-updater')][string]$Scenario,
    [Parameter(Mandatory = $true)][string]$Repo,
    [Parameter(Mandatory = $true)][string]$NewZip,
    [switch]$Prepare,
    [switch]$Launch
)
$ErrorActionPreference = 'Stop'
if ($Root -match 'OneDrive') { throw 'Refusing to use OneDrive.' }
$S = Join-Path $Root $Scenario
$Desktop = Join-Path $S 'Desktop'; $Downloads = Join-Path $S 'Downloads'; $Docs = Join-Path $S 'Documents'
$Start = Join-Path $S 'StartMenu\Programs'; $Local = Join-Path $S 'AppData\Local'; $Temp = Join-Path $S 'Temp'
$Install = Join-Path $Local 'Programs\FerricDAW'
$OldZip12 = Join-Path $Repo 'dist\remote-v1.1.12.zip'
$Zip13 = Join-Path $Repo 'target\release-verification\public-v1.1.13\FerricDAW-v1.1.13-win-x64.zip'
$Zip14 = Join-Path $Repo 'target\release-verification\public-v1.1.14\FerricDAW-v1.1.14-win-x64.zip'

function Set-SandboxEnv {
    $env:LOCALAPPDATA = $Local; $env:TEMP = $Temp; $env:TMP = $Temp
    $env:FERRIC_DESKTOP_DIR = $Desktop; $env:FERRIC_DOWNLOADS_DIR = $Downloads
    $env:FERRIC_DOCUMENTS_DIR = $Docs; $env:FERRIC_START_MENU_DIR = $Start; $env:FERRIC_PROFILE_DIR = $S
}
function New-Lnk($Path, $Target) {
    $s = (New-Object -ComObject WScript.Shell).CreateShortcut($Path)
    $s.TargetPath = $Target; $s.WorkingDirectory = (Split-Path $Target); $s.Save()
}
# Exact v1.1.12 helper (crates/ferric-app/src/update.rs @ v1.1.12) with a no-op wait.
function Invoke-Updater112($Zip, $Dest) {
    $helper = Join-Path $Temp 'ferricdaw-apply-update.ps1'
    $q = { param($v) "'" + $v.Replace("'", "''") + "'" }
    $script = @"
`$ErrorActionPreference = 'Stop'
`$zip = $(& $q $Zip)
`$dest = $(& $q $Dest)
`$exeName = 'ferricdaw.exe'
`$stage = Join-Path `$env:TEMP ("ferricdaw-update-stage-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path `$stage | Out-Null
Expand-Archive -LiteralPath `$zip -DestinationPath `$stage -Force
`$kids = Get-ChildItem -LiteralPath `$stage
if (`$kids.Count -eq 1 -and `$kids[0].PSIsContainer) { `$src = `$kids[0].FullName } else { `$src = `$stage }
Copy-Item -Path (Join-Path `$src '*') -Destination `$dest -Recurse -Force
Remove-Item `$stage -Recurse -Force -ErrorAction SilentlyContinue
"@
    [IO.File]::WriteAllText($helper, $script)
    Copy-Item $Zip (Join-Path $Temp ('FerricDAW-v' + ([regex]::Match((Split-Path -Leaf $Zip), '\d+\.\d+\.\d+').Value) + '-update.zip'))
    powershell -NoProfile -ExecutionPolicy Bypass -File $helper
    if ($LASTEXITCODE -ne 0) { throw 'v1.1.12 helper failed' }
}
function Invoke-Updater($Ps1, $Zip, $Dest, $Extra) {
    $dummy = Start-Process -FilePath powershell -ArgumentList '-NoProfile', '-Command', 'exit' -PassThru -WindowStyle Hidden
    $dummy.WaitForExit()
    $copy = Join-Path $Temp ('FerricDAW-update-' + [guid]::NewGuid() + '.zip'); Copy-Item $Zip $copy
    $hash = (Get-FileHash $copy).Hash
    $args = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $Ps1, '-Zip', $copy, '-Destination', $Dest, '-ExeName', 'ferricdaw.exe', '-AppPid', $dummy.Id, '-Sha256', $hash) + $Extra
    & powershell @args
    if ($LASTEXITCODE -ne 0) { throw "updater $Ps1 failed" }
}

if ($Prepare) {
    if (Test-Path $S) { Remove-Item -LiteralPath $S -Recurse -Force }
    foreach ($d in $Desktop, $Downloads, $Docs, $Start, $Local, $Temp) { New-Item -ItemType Directory -Force -Path $d | Out-Null }
    Set-SandboxEnv
    # The user's own things on the Desktop.
    New-Item -ItemType Directory -Force (Join-Path $Desktop 'plugins\MyVerb.vst3') | Out-Null
    Set-Content (Join-Path $Desktop 'plugins\MyVerb.vst3\MyVerb.vst3') 'user plugin'
    Set-Content (Join-Path $Desktop 'My Song.ferric') 'project'
    Set-Content (Join-Path $Desktop 'shopping list.txt') 'milk'
    Set-Content (Join-Path $Desktop 'notes.txt') 'user notes'
    # An older full install in Downloads with a Start Menu shortcut and a user plugin.
    $old = Join-Path $Downloads 'FerricDAW-v1.1.12-win-x64'
    Expand-Archive -LiteralPath $OldZip12 -DestinationPath $old
    New-Item -ItemType Directory -Force (Join-Path $old 'plugins') | Out-Null
    Set-Content (Join-Path $old 'plugins\Grit.clap') 'user clap'
    New-Lnk (Join-Path $Start 'FerricDAW.lnk') (Join-Path $old 'ferricdaw.exe')
    # User settings that must survive.
    New-Item -ItemType Directory -Force (Join-Path $Local 'FerricDAW') | Out-Null
    Set-Content (Join-Path $Local 'FerricDAW\recent_projects.txt') (Join-Path $Desktop 'My Song.ferric')
    Set-Content (Join-Path $Local 'FerricDAW\first_run_done') '1'

    # The running exe was moved to the Desktop on its own.
    $tmp = Join-Path $Temp 'x12'; Expand-Archive -LiteralPath $OldZip12 -DestinationPath $tmp
    Copy-Item (Join-Path $tmp 'ferricdaw.exe') $Desktop; Remove-Item $tmp -Recurse -Force
    New-Lnk (Join-Path $Desktop 'FerricDAW.lnk') (Join-Path $Desktop 'ferricdaw.exe')
    switch ($Scenario) {
        'from-1.1.12' { Invoke-Updater112 $NewZip $Desktop }
        'from-1.1.13' {
            Invoke-Updater112 $Zip13 $Desktop          # the reported v1.1.12 -> v1.1.13 dump
            $ps13 = Join-Path $Temp 'apply_update_113.ps1'
            git -C $Repo show v1.1.13:crates/ferric-app/src/apply_update.ps1 | Set-Content -Encoding UTF8 $ps13
            Invoke-Updater $ps13 $NewZip $Desktop @() # v1.1.13's updater -> v1.1.14 onto the Desktop
            Get-Process ferricdaw -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$S*" } | Stop-Process
        }
        'from-1.1.14' {
            # A v1.1.14 user: installed in its own folder (with a Start Menu
            # shortcut), updated by v1.1.14's own helper. The stray v1.1.12
            # Desktop exe above is what an older copy left behind.
            Expand-Archive -LiteralPath $Zip14 -DestinationPath $Install
            New-Lnk (Join-Path $Start 'FerricDAW 1.1.14.lnk') (Join-Path $Install 'ferricdaw.exe')
            $ps14 = Join-Path $Temp 'apply_update_114.ps1'
            git -C $Repo show v1.1.14:crates/ferric-app/src/apply_update.ps1 | Set-Content -Encoding UTF8 $ps14
            Invoke-Updater $ps14 $NewZip $Install @()
            Get-Process ferricdaw -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$S*" } | Stop-Process
            "== Install folder after v1.1.14's updater"; Get-ChildItem -LiteralPath $Install -Force | ForEach-Object Name
        }
        'new-updater' {
            # v1.1.14's own helper when the running copy is on the Desktop.
            Invoke-Updater112 $NewZip $Desktop
            # The helper deletes itself after a successful update, as in the app, so run a temp copy.
            $ps14 = Join-Path $Temp 'ferricdaw-apply-update-1.1.14.ps1'
            Copy-Item (Join-Path $Repo 'crates\ferric-app\src\apply_update.ps1') $ps14
            Invoke-Updater $ps14 $NewZip $Install @('-RelocatedFrom', $Desktop)
        }
    }
    "== Desktop after the old updater"; Get-ChildItem -LiteralPath $Desktop -Recurse -Force | ForEach-Object { $_.FullName.Substring($Desktop.Length + 1) }
}
if ($Launch) {
    Set-SandboxEnv
    $from = if ($Scenario -eq 'from-1.1.14') { $Install } else { $Desktop }
    $exe = Join-Path $from 'ferricdaw.exe'
    $p = Start-Process -FilePath $exe -WorkingDirectory $from -PassThru -WindowStyle Maximized
    "launched $($p.Id)"
}

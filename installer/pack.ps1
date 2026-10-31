<#
.SYNOPSIS
  Packs Tingskriver for Windows with Velopack: Tingskriver-Setup.exe, the
  full and delta packages and the update feed (releases.win.json).

.DESCRIPTION
  Copies the app exe (and any extra files) into a staging folder, runs
  `vpk pack` and collects the files that belong on the GitHub Release in
  <OutputDir>\upload. A full package of the previous version in OutputDir
  (from `vpk download github`) makes vpk build a delta package as well.

  Needs the vpk tool in the version the app's velopack crate uses:
    dotnet tool install -g vpk --version 1.2.161

.EXAMPLE
  # GPUI app, as CI does it
  pwsh installer/pack.ps1 -Version 0.5.1-dev.1 -Exe target/release/tingskriver-app.exe

.EXAMPLE
  # The Tauri app (migration release); its exe has no Velopack hooks
  pwsh installer/pack.ps1 -Version 0.5.1 -Exe target/x86_64-pc-windows-msvc/release/tingskriver.exe `
    -Extra frontend/src-tauri/binaries/ffmpeg.exe,frontend/src-tauri/binaries/llama-helper.exe `
    -SkipVeloAppCheck
#>
param(
  [Parameter(Mandatory = $true)] [string] $Version,
  # The main exe. Its file name becomes the shortcut target.
  [Parameter(Mandatory = $true)] [string] $Exe,
  # More files or folders next to the exe (sidecars, DLLs).
  [string[]] $Extra = @(),
  [string] $OutputDir = "installer/Releases",
  # Never change after the first release: updates and the install folder
  # (%LOCALAPPDATA%\TingskriverApp) are keyed by it. It differs from the
  # Tauri app's folder and uninstall key (Tingskriver), so both can be
  # installed during the move.
  [string] $PackId = "TingskriverApp",
  [string] $Title = "Tingskriver",
  [string] $Channel = "win",
  [string] $ReleaseNotes = "",
  [switch] $SkipVeloAppCheck
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repo = Resolve-Path (Join-Path $PSScriptRoot "..")
$icon = Join-Path $repo "frontend/src-tauri/icons/icon.ico"
$exePath = Resolve-Path $Exe
$mainExe = Split-Path $exePath -Leaf

if ($Version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$') {
  throw "Version '$Version' is not semver (0.5.1 or 0.5.1-dev.1)"
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
$OutputDir = (Resolve-Path $OutputDir).Path
$staging = Join-Path ([IO.Path]::GetTempPath()) "tingskriver-pack-$Version"
if (Test-Path $staging) { Remove-Item -Recurse -Force $staging }
New-Item -ItemType Directory -Path $staging | Out-Null

Copy-Item $exePath -Destination $staging
foreach ($item in $Extra) {
  Copy-Item (Resolve-Path $item) -Destination $staging -Recurse
}
foreach ($doc in @("LICENSE.md", "NOTICE.md", "THIRD_PARTY_LICENSES.md")) {
  Copy-Item (Join-Path $repo $doc) -Destination $staging
}
Write-Host "Staged files:"
Get-ChildItem -Recurse $staging | ForEach-Object { Write-Host "  $($_.FullName.Substring($staging.Length + 1)) ($($_.Length) bytes)" }

$vpkArgs = @(
  "pack",
  "--packId", $PackId,
  "--packVersion", $Version,
  "--packDir", $staging,
  "--mainExe", $mainExe,
  "--packTitle", $Title,
  "--packAuthors", "Mahope",
  "--icon", $icon,
  "--channel", $Channel,
  "--outputDir", $OutputDir,
  "--shortcuts", "Desktop,StartMenuRoot",
  "--noPortable"
)
if ($ReleaseNotes) { $vpkArgs += @("--releaseNotes", (Resolve-Path $ReleaseNotes).Path) }
if ($SkipVeloAppCheck) { $vpkArgs += "--skipVeloAppCheck" }

Write-Host "vpk $($vpkArgs -join ' ')"
& vpk @vpkArgs
if ($LASTEXITCODE -ne 0) { throw "vpk pack failed with exit code $LASTEXITCODE" }

# The files for this release only. The feed is cut down to this version's
# packages: the app reads the feeds of the newest releases and joins them,
# so an older full package downloaded for the delta must not be listed twice.
$upload = Join-Path $OutputDir "upload"
if (Test-Path $upload) { Remove-Item -Recurse -Force $upload }
New-Item -ItemType Directory -Path $upload | Out-Null

$setup = Join-Path $OutputDir "$PackId-$Channel-Setup.exe"
if (-not (Test-Path $setup)) { throw "vpk did not produce $setup" }
Copy-Item $setup -Destination (Join-Path $upload "Tingskriver-Setup.exe")

$feedName = "releases.$Channel.json"
$feed = Get-Content -Raw (Join-Path $OutputDir $feedName) | ConvertFrom-Json
$assets = @($feed.Assets | Where-Object { $_.Version -eq $Version })
if (-not ($assets | Where-Object { $_.Type -eq 'Full' })) {
  throw "$feedName has no full package for $Version"
}
foreach ($asset in $assets) {
  Copy-Item (Join-Path $OutputDir $asset.FileName) -Destination $upload
}
$feed.Assets = $assets
$json = ConvertTo-Json -InputObject $feed -Depth 8
[IO.File]::WriteAllText((Join-Path $upload $feedName), $json, (New-Object Text.UTF8Encoding $false))

Write-Host "Release files in ${upload}:"
Get-ChildItem $upload | ForEach-Object { Write-Host ("  {0} ({1:N1} MB)" -f $_.Name, ($_.Length / 1MB)) }
$deltas = @($assets | Where-Object { $_.Type -eq 'Delta' })
if ($deltas.Count -eq 0) {
  Write-Host "No delta package (no earlier full package in $OutputDir)."
}

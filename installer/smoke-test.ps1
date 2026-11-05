<#
.SYNOPSIS
  Smoke test of the Windows installer: silent install, uninstall entry,
  --diagnose, the app staying alive, silent uninstall, and that user data
  and keys survive it. Used by .github/workflows/installer-smoke.yml.

.DESCRIPTION
  Refuses to run on a machine that already has the app installed, so it
  never touches a developer's installation. Seeds synthetic user data and a
  Credential Manager key, which must still be there after the uninstall.
  Everything useful for a failure (installer output, diagnose JSON, the app
  log, the newest Application event log errors) is copied to OutputDir.

  Idea and steps follow anarlog's desktop_ci.yaml (MIT, Fastrepl, Inc.,
  see NOTICE.md); the code is our own.

.EXAMPLE
  pwsh installer/smoke-test.ps1 -Setup installer/Releases/upload/Tingskriver-Setup.exe -OutputDir smoke-out
#>
param(
  [Parameter(Mandatory = $true)] [string] $Setup,
  [string] $OutputDir = "smoke-out",
  [int] $InstallTimeoutSeconds = 180,
  [int] $UninstallTimeoutSeconds = 180,
  [int] $KeepAliveSeconds = 20
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$packId = 'TingskriverApp'
$installDir = Join-Path $env:LOCALAPPDATA $packId
$uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$packId"
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$dataDir = Join-Path $env:APPDATA 'dk.mahoje.tingskriver'
$logFile = Join-Path $env:LOCALAPPDATA 'dk.mahoje.tingskriver\logs\tingskriver-app.log'
$credentialTarget = 'Tingskriver/smoke-test'
$dataMarker = Join-Path $dataDir 'smoke-test-keep.txt'

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
$OutputDir = (Resolve-Path $OutputDir).Path

function Step([string] $text) { Write-Host "::group::$text"; Write-Host $text }
function EndStep { Write-Host '::endgroup::' }
function Fail([string] $text) { throw "SMOKE TEST FAILED: $text" }

function Read-LogText {
  if (Test-Path $logFile) { return (Get-Content -Raw -Path $logFile) }
  return ''
}

# Waits for a condition; returns whether it became true within the time.
function Wait-Until([scriptblock] $condition, [int] $seconds) {
  $deadline = (Get-Date).AddSeconds($seconds)
  while ((Get-Date) -lt $deadline) {
    if (& $condition) { return $true }
    Start-Sleep -Milliseconds 500
  }
  return [bool](& $condition)
}

# Starts a program, waits for it with a timeout, and returns its exit code.
# stdout and stderr go to files in OutputDir (printed on failure).
function Invoke-Timed([string] $file, [string[]] $arguments, [string] $name, [int] $seconds) {
  $out = Join-Path $OutputDir "$name.stdout.txt"
  $err = Join-Path $OutputDir "$name.stderr.txt"
  $params = @{ FilePath = $file; PassThru = $true; RedirectStandardOutput = $out; RedirectStandardError = $err }
  if ($arguments.Count -gt 0) { $params.ArgumentList = $arguments }
  $process = Start-Process @params
  if (-not $process.WaitForExit($seconds * 1000)) {
    try { $process.Kill() } catch { }
    Fail "$name did not finish within $seconds s"
  }
  return $process.ExitCode
}

function Save-Diagnostics {
  Write-Host '--- Diagnostics ---'
  foreach ($file in Get-ChildItem -Path $OutputDir -Filter '*.txt' -ErrorAction SilentlyContinue) {
    Write-Host "--- $($file.Name) ---"
    Get-Content -Path $file.FullName -Tail 60 -ErrorAction SilentlyContinue
  }
  if (Test-Path $logFile) {
    Copy-Item $logFile -Destination (Join-Path $OutputDir 'tingskriver-app.log') -Force
    Write-Host '--- tingskriver-app.log (tail) ---'
    Get-Content -Path $logFile -Tail 80
  } else {
    Write-Host "No app log at $logFile"
  }
  try {
    $events = Get-WinEvent -LogName Application -MaxEvents 25 -FilterXPath '*[System[(Level=1 or Level=2)]]' -ErrorAction Stop |
      Select-Object TimeCreated, ProviderName, Id, LevelDisplayName, Message
    $events | Format-List | Out-String | Set-Content -Path (Join-Path $OutputDir 'eventlog-tail.txt') -Encoding utf8
    Write-Host '--- Application event log, newest errors ---'
    $events | Format-List | Out-String | Write-Host
  } catch {
    "No error events found, or the event log could not be read: $($_.Exception.Message)" |
      Set-Content -Path (Join-Path $OutputDir 'eventlog-tail.txt') -Encoding utf8
  }
}

$app = $null
try {
  # 1. Never run on top of an existing installation.
  Step 'Refuse to run on an existing installation'
  if ((Test-Path $installDir) -or (Test-Path $uninstallKey)) {
    Fail "The app is already installed ($installDir or $uninstallKey exists). Run this on a clean runner or VM."
  }
  if (Test-Path (Join-Path $dataDir 'slet-data-ved-afinstallation')) {
    Fail 'The delete-data marker exists; this machine is not clean.'
  }
  $setupPath = (Resolve-Path $Setup).Path
  $hash = (Get-FileHash $setupPath -Algorithm SHA256).Hash.ToLowerInvariant()
  Write-Host "Installer: $setupPath ($((Get-Item $setupPath).Length) bytes) sha256 $hash"
  EndStep

  # 2. Synthetic user data and a key that must survive the uninstall.
  Step 'Seed synthetic user data and a Credential Manager key'
  New-Item -ItemType Directory -Force -Path $dataDir | Out-Null
  Set-Content -Path $dataMarker -Value 'synthetic smoke-test data' -Encoding ascii
  $secret = [guid]::NewGuid().ToString('N')
  & cmdkey.exe "/generic:$credentialTarget" '/user:smoke-test' "/pass:$secret" | Out-Null
  if ($LASTEXITCODE -ne 0) { Fail "cmdkey could not add $credentialTarget" }
  $secret = $null
  EndStep

  # 3. Silent install.
  Step 'Silent install'
  $code = Invoke-Timed $setupPath @('--silent') 'install' $InstallTimeoutSeconds
  Write-Host "Setup exit code $code"
  if ($code -ne 0) { Fail "Setup.exe --silent exited with $code" }
  if (-not (Wait-Until { Test-Path $uninstallKey } 60)) { Fail "No uninstall entry $uninstallKey after install" }
  $exe = Join-Path $installDir 'current\tingskriver-app.exe'
  $update = Join-Path $installDir 'Update.exe'
  if (-not (Test-Path $exe)) { Fail "$exe is missing after install" }
  if (-not (Test-Path $update)) { Fail "$update is missing after install" }
  $entry = Get-ItemProperty $uninstallKey
  Write-Host "Uninstall entry: $($entry.DisplayName) $($entry.DisplayVersion)"
  EndStep

  # 4. The after-install hook: start with Windows, and its log lines.
  Step 'After-install hook'
  $expectedRun = "`"$exe`" --hidden"
  $run = (Get-ItemProperty $runKey -ErrorAction SilentlyContinue).PSObject.Properties['Tingskriver']
  if (-not $run) { Fail 'HKCU Run value Tingskriver is missing after install' }
  if ($run.Value -ne $expectedRun) { Fail "Run value is '$($run.Value)', expected '$expectedRun'" }
  if (-not (Wait-Until { (Read-LogText) -match 'installer: installed' } 30)) {
    Fail "The app log has no 'installer: installed' line ($logFile)"
  }
  if ((Read-LogText) -notmatch 'installer install:') {
    Write-Host "::warning::The app log has no 'installer install:' line"
  }
  EndStep

  # 5. Diagnose. Findings of kind error (no Vulkan on a runner, say) give
  #    exit code 1 and are not a failed installation; any other exit code,
  #    or output that is not JSON, is.
  Step 'tingskriver-app --diagnose --json'
  $code = Invoke-Timed $exe @('--diagnose', '--json') 'diagnose' 60
  Write-Host "Diagnose exit code $code"
  if ($code -ne 0 -and $code -ne 1) { Fail "--diagnose exited with $code" }
  $diagnosePath = Join-Path $OutputDir 'diagnose.stdout.txt'
  $diagnoseText = Get-Content -Raw -Path $diagnosePath
  try { $diagnose = $diagnoseText | ConvertFrom-Json } catch { Fail "--diagnose did not print JSON: $($_.Exception.Message)" }
  Copy-Item $diagnosePath -Destination (Join-Path $OutputDir 'diagnose.json') -Force
  Write-Host $diagnoseText
  if ($code -eq 1) { Write-Host '::warning::--diagnose reported errors (expected on a runner without a GPU or audio); see diagnose.json' }
  EndStep

  # 6. Start hidden and stay alive.
  Step "Start the app hidden and keep it alive for $KeepAliveSeconds s"
  $app = Start-Process -FilePath $exe -ArgumentList '--hidden' -PassThru
  $deadline = (Get-Date).AddSeconds($KeepAliveSeconds)
  while ((Get-Date) -lt $deadline) {
    if ($app.HasExited) { Fail "The app exited after $([int]((Get-Date) - $app.StartTime).TotalSeconds) s with code $($app.ExitCode)" }
    Start-Sleep -Seconds 1
  }
  if ($app.HasExited) { Fail "The app exited with code $($app.ExitCode)" }
  if ((Read-LogText) -notmatch '\(GPUI\) starting hidden') { Fail "The app log has no '(GPUI) starting hidden' line" }
  Write-Host "The app is still running (pid $($app.Id))."
  Stop-Process -Id $app.Id -Force
  if (-not $app.WaitForExit(15000)) { Fail 'The app process did not stop' }
  $app = $null
  EndStep

  # 7. Silent uninstall, with the command Windows itself would run.
  Step 'Silent uninstall'
  $entry = Get-ItemProperty $uninstallKey
  $quiet = $entry.PSObject.Properties['QuietUninstallString']
  $command = if ($quiet -and $quiet.Value) { $quiet.Value } else { "$($entry.UninstallString) --silent" }
  Write-Host "Uninstall command: $command"
  # The command is `"path" args`; run it through cmd so the quoting is kept.
  $code = Invoke-Timed 'cmd.exe' @('/c', "`"$command`"") 'uninstall' $UninstallTimeoutSeconds
  Write-Host "Uninstall exit code $code"
  if ($code -ne 0) { Fail "The uninstaller exited with $code" }
  if (-not (Wait-Until { -not (Test-Path $uninstallKey) } 60)) { Fail "$uninstallKey is still there after uninstall" }
  # Velopack removes the folder after the uninstaller process has exited.
  if (-not (Wait-Until { -not (Test-Path $installDir) } 60)) {
    Get-ChildItem -Recurse -Force $installDir -ErrorAction SilentlyContinue | Select-Object -First 40 FullName | Format-Table | Out-String | Write-Host
    Fail "$installDir is still there after uninstall"
  }
  $run = (Get-ItemProperty $runKey -ErrorAction SilentlyContinue).PSObject.Properties['Tingskriver']
  if ($run) { Fail "HKCU Run value Tingskriver is still there after uninstall: $($run.Value)" }
  EndStep

  # 8. User data and keys stay (installer/README.md, "Hooks").
  Step 'User data and Credential Manager keys stay'
  if (-not (Test-Path $dataMarker)) { Fail "User data was deleted: $dataMarker is gone" }
  $keys = & cmdkey.exe /list 2>&1 | Out-String
  if ($keys -notmatch [regex]::Escape($credentialTarget)) { Fail "Credential Manager key $credentialTarget was deleted" }
  Write-Host 'User data and the Credential Manager key are still there.'
  EndStep

  Write-Host "SMOKE TEST PASSED (installer sha256 $hash)"
  exit 0
}
catch {
  Write-Host "::error::$($_.Exception.Message)"
  Save-Diagnostics
  exit 1
}
finally {
  if ($app -and -not $app.HasExited) { try { $app.Kill() } catch { } }
  # Leave nothing behind on the runner.
  & cmdkey.exe "/delete:$credentialTarget" 2>&1 | Out-Null
  Remove-Item -Force -ErrorAction SilentlyContinue $dataMarker
}

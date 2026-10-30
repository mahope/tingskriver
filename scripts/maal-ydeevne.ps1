<#
.SYNOPSIS
  Måler ydeevne for den installerede Tingskriver-app (koldstart, varmstart, hukommelse, dikteringstid).

.DESCRIPTION
  Scriptet ændrer intet. Det starter og lukker kun appen (CloseMainWindow, derefter
  Stop-Process efter 10 s), læser logfiler og læser en kopi af dikteringshistorikken.
  Der printes kun tal, aldrig tekstindhold.

  Kør:  powershell -ExecutionPolicy Bypass -File scripts\maal-ydeevne.ps1
  Se docs/ydeevne.md for metode og begrænsninger.

.PARAMETER Repetitioner
  Antal gentagelser pr. måling. Medianen bruges. Standard 5.

.PARAMETER EfterMinutter
  Mål hukommelsen igen N minutter efter første tomgangsmåling (fx i tray). 0 = spring over.

.PARAMETER Dikteringer
  Antal seneste dikteringer der indgår i p50/p95. Standard 50.

.PARAMETER ExeSti
  Sti til appen. Standard %LOCALAPPDATA%\Tingskriver\tingskriver.exe.

.PARAMETER TomgangSekunder
  Ventetid fra første synlige vindue til hukommelsesmåling. Standard 60.

.PARAMETER KunDiktering
  Mål kun dikteringstid (starter ikke appen).

.PARAMETER KunKoldstart
  Mål én koldstart og afslut. Kør efter en genstart af pc'en; brug -KoldLog for at samle
  flere kørsler (én tekstlinje pr. kørsel) og få medianen.

.PARAMETER KunHukommelse
  Mål hukommelse og CPU på en allerede kørende app (fx lige efter et møde) og afslut.
  Starter, lukker og rører ikke appen.

.PARAMETER KoldLog
  Valgfri sti til en tekstfil som -KunKoldstart tilføjer sin måling til. Er det eneste scriptet skriver.
#>
param(
    [int]$Repetitioner = 5,
    [int]$EfterMinutter = 0,
    [int]$Dikteringer = 50,
    [string]$ExeSti = (Join-Path $env:LOCALAPPDATA 'Tingskriver\tingskriver.exe'),
    [int]$TomgangSekunder = 60,
    [switch]$KunDiktering,
    [switch]$KunKoldstart,
    [switch]$KunHukommelse,
    [string]$KoldLog
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$AppId = 'dk.mahoje.tingskriver'
$ProcessNavn = 'tingskriver'
$VinduTimeoutMs = 60000
$LukTimeoutSek = 10

# ---------------------------------------------------------------- hjælpere

function Get-Median([double[]]$Values) {
    if ($null -eq $Values -or $Values.Count -eq 0) { return $null }
    $s = @($Values | Sort-Object)
    $n = $s.Count
    if ($n % 2 -eq 1) { return [double]$s[($n - 1) / 2] }
    return ([double]$s[$n / 2 - 1] + [double]$s[$n / 2]) / 2
}

# Nearest-rank percentil (p mellem 0 og 100).
function Get-Percentile([double[]]$Values, [double]$P) {
    if ($null -eq $Values -or $Values.Count -eq 0) { return $null }
    $s = @($Values | Sort-Object)
    $rank = [math]::Ceiling($P / 100 * $s.Count)
    if ($rank -lt 1) { $rank = 1 }
    return [double]$s[$rank - 1]
}

function Format-Num($v, [int]$Decimals = 0) {
    if ($null -eq $v) { return 'n/a' }
    return ([double]$v).ToString("N$Decimals", [System.Globalization.CultureInfo]::GetCultureInfo('da-DK'))
}

function Get-AppProcesses {
    return @(Get-Process -Name $ProcessNavn -ErrorAction SilentlyContinue)
}

# ---------------------------------------------------------------- Win32

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class TsWin {
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
}
'@

# ---------------------------------------------------------------- sikkerhedstjek

function Get-RecordingFolders {
    $folders = @()
    $music = [Environment]::GetFolderPath('MyMusic')
    if ($music) { $folders += (Join-Path $music 'tingskriver-recordings') }
    $folders += (Join-Path $env:USERPROFILE 'Music\tingskriver-recordings')
    # Brugerens valgte mappe (save_folder) står i appens store-fil; den læses kun, aldrig skrives.
    $prefs = Join-Path (Join-Path $env:APPDATA $AppId) 'recording_preferences.json'
    if (Test-Path -LiteralPath $prefs) {
        try {
            $j = Get-Content -LiteralPath $prefs -Raw -Encoding UTF8 | ConvertFrom-Json
            $custom = $j.preferences.save_folder
            if ($custom) { $folders += [string]$custom }
        } catch { Write-Warning 'Kunne ikke læse recording_preferences.json; tjekker kun standardmappen.' }
    }
    return @($folders | Select-Object -Unique | Where-Object { Test-Path -LiteralPath $_ })
}

# Stopper hvis filer i optagelsesmappen er ændret de seneste 2 minutter.
function Assert-NoRecordingInProgress {
    $cutoff = (Get-Date).AddMinutes(-2)
    foreach ($f in (Get-RecordingFolders)) {
        $recent = @(Get-ChildItem -LiteralPath $f -Recurse -File -ErrorAction SilentlyContinue |
            Where-Object { $_.LastWriteTime -gt $cutoff })
        if ($recent.Count -gt 0) {
            Write-Warning "Der ser ud til at foregå en optagelse: $($recent.Count) fil(er) i '$f' er ændret de seneste 2 minutter."
            Write-Warning 'Scriptet stopper uden at have startet eller lukket noget. Vent til optagelsen er færdig og kør igen.'
            exit 2
        }
    }
}

function Assert-AppNotRunning {
    $running = Get-AppProcesses
    if ($running.Count -gt 0) {
        Write-Warning 'tingskriver.exe kører allerede. Scriptet rører ikke en kørende app (der kan være en optagelse i gang).'
        Write-Warning 'Luk appen selv (også fra tray) og kør igen.'
        exit 3
    }
}

# ---------------------------------------------------------------- start / luk

# Starter appen og returnerer @{ Ms; Process } når hovedvinduet er synligt, ellers Ms = $null.
function Start-AppTimed {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $proc = Start-Process -FilePath $ExeSti -PassThru
    $ms = $null
    while ($sw.ElapsedMilliseconds -lt $VinduTimeoutMs) {
        $proc.Refresh()
        if ($proc.HasExited) { break }
        $h = $proc.MainWindowHandle
        if ($h -ne [IntPtr]::Zero -and [TsWin]::IsWindowVisible($h)) {
            $ms = $sw.Elapsed.TotalMilliseconds
            break
        }
        Start-Sleep -Milliseconds 10
    }
    return @{ Ms = $ms; Process = $proc }
}

# Appen skjuler sig i tray ved CloseMainWindow (CloseRequested -> prevent_close), så processen
# afsluttes normalt ikke af sig selv. Fallback: Stop-Process på appen og alle dens efterkommere,
# så llama-helper/ffmpeg ikke efterlades som forældreløse. Varmstart måles derfor efter et kill.
function Stop-AppGracefully($Proc) {
    $roots = @(Get-AppProcesses | ForEach-Object { $_.Id })
    foreach ($p in (Get-AppProcesses)) {
        [void]$p.CloseMainWindow()
    }
    $deadline = (Get-Date).AddSeconds($LukTimeoutSek)
    while ((Get-Date) -lt $deadline -and (Get-AppProcesses).Count -gt 0) {
        Start-Sleep -Milliseconds 200
    }
    $left = Get-AppProcesses
    if ($left.Count -gt 0) {
        Write-Host "  (appen lukkede ikke selv inden $LukTimeoutSek s - den skjuler sig i tray - bruger Stop-Process)"
        $ids = @(Get-ProcessTreeIds $roots)
        Stop-Process -Id $ids -Force -ErrorAction SilentlyContinue
        Start-Sleep -Milliseconds 500
    }
    # Underprocesser (webview2) lukker typisk med forælderen; giv dem et øjeblik.
    Start-Sleep -Seconds 2
}

# ---------------------------------------------------------------- hukommelse

# Procestræ: appen og alle efterkommere (msedgewebview2, llama-helper, ffmpeg osv.).
function Get-ProcessTreeIds([int[]]$RootIds) {
    $all = @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId)
    $ids = New-Object 'System.Collections.Generic.HashSet[int]'
    foreach ($r in $RootIds) { [void]$ids.Add($r) }
    $changed = $true
    while ($changed) {
        $changed = $false
        foreach ($p in $all) {
            if ($ids.Contains([int]$p.ParentProcessId) -and -not $ids.Contains([int]$p.ProcessId)) {
                [void]$ids.Add([int]$p.ProcessId)
                $changed = $true
            }
        }
    }
    return @($ids)
}

# Returnerer MB for working set og private bytes pr. gruppe, samt antal processer.
function Get-MemorySample {
    $roots = @(Get-AppProcesses | ForEach-Object { $_.Id })
    $res = @{
        AppWS = 0.0; AppPB = 0.0
        WebViewWS = 0.0; WebViewPB = 0.0; WebViewCount = 0
        AndreWS = 0.0; AndrePB = 0.0; AndreCount = 0
        TotalWS = 0.0; TotalPB = 0.0
        CpuSeconds = 0.0
    }
    if ($roots.Count -eq 0) { return $res }
    foreach ($id in (Get-ProcessTreeIds $roots)) {
        $p = Get-Process -Id $id -ErrorAction SilentlyContinue
        if (-not $p) { continue }
        $ws = $p.WorkingSet64 / 1MB
        $pb = $p.PrivateMemorySize64 / 1MB
        $cpu = 0.0
        try { $cpu = $p.TotalProcessorTime.TotalSeconds } catch { }
        $res.CpuSeconds += $cpu
        $name = $p.ProcessName.ToLowerInvariant()
        if ($name -eq $ProcessNavn) {
            $res.AppWS += $ws; $res.AppPB += $pb
        } elseif ($name -eq 'msedgewebview2') {
            $res.WebViewWS += $ws; $res.WebViewPB += $pb; $res.WebViewCount++
        } else {
            $res.AndreWS += $ws; $res.AndrePB += $pb; $res.AndreCount++
        }
        $res.TotalWS += $ws; $res.TotalPB += $pb
    }
    return $res
}

# Tomgangsmåling: venter, og måler CPU over de sidste 10 s af ventetiden.
function Measure-Idle([int]$WaitSeconds) {
    $cpuWindow = [math]::Min(10, $WaitSeconds)
    if ($WaitSeconds -gt $cpuWindow) { Start-Sleep -Seconds ($WaitSeconds - $cpuWindow) }
    $before = Get-MemorySample
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    if ($cpuWindow -gt 0) { Start-Sleep -Seconds $cpuWindow }
    $after = Get-MemorySample
    $elapsed = [math]::Max($sw.Elapsed.TotalSeconds, 0.001)
    $cores = [Environment]::ProcessorCount
    $after.CpuPct = [math]::Max(0.0, ($after.CpuSeconds - $before.CpuSeconds) / $elapsed / $cores * 100)
    return $after
}

# ---------------------------------------------------------------- dikteringstid

function Get-LogLatencies([int]$Max) {
    $dir = Join-Path $env:LOCALAPPDATA "$AppId\logs"
    $files = @('tingskriver.log.2', 'tingskriver.log.1', 'tingskriver.log') |
        ForEach-Object { Join-Path $dir $_ } | Where-Object { Test-Path -LiteralPath $_ }
    $re = [regex]'Dictation: (\d+) ms from release to text \(engine (\w+), (\d+) ms audio, \d+ chars, app .*, method (\w+)\)\s*$'
    $rows = New-Object System.Collections.Generic.List[object]
    foreach ($f in $files) {
        # FileShare.ReadWrite: appen skriver måske stadig til filen.
        $fs = New-Object System.IO.FileStream($f, 'Open', 'Read', 'ReadWrite')
        $sr = New-Object System.IO.StreamReader($fs, [System.Text.Encoding]::UTF8)
        try {
            while (-not $sr.EndOfStream) {
                $m = $re.Match($sr.ReadLine())
                if ($m.Success -and $m.Groups[4].Value -ne 'none') {
                    $rows.Add([pscustomobject]@{
                        LatencyMs = [double]$m.Groups[1].Value
                        AudioMs   = [double]$m.Groups[3].Value
                    })
                }
            }
        } finally { $sr.Dispose() }
    }
    if ($rows.Count -gt $Max) { return @($rows | Select-Object -Last $Max) }
    return @($rows)
}

# Kører en SELECT mod en kopi af dictation.sqlite. Returnerer @{ Status; Lines }.
function Invoke-DictationQuery([string]$Query) {
    $dbName = 'dictation.sqlite'
    $db = $null
    foreach ($base in @($env:APPDATA, $env:LOCALAPPDATA)) {
        $cand = Join-Path (Join-Path $base $AppId) $dbName
        if (Test-Path -LiteralPath $cand) { $db = $cand; break }
    }
    if (-not $db) { return @{ Status = "$dbName findes ikke"; Lines = @() } }
    $sqlite = Get-Command sqlite3.exe -ErrorAction SilentlyContinue
    if (-not $sqlite) { return @{ Status = 'sqlite3.exe ikke på PATH (springes over)'; Lines = @() } }
    # Arbejd på en kopi, så den levende database (WAL) aldrig røres.
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("tingskriver-maal-" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        foreach ($suffix in @('', '-wal', '-shm')) {
            $src = $db + $suffix
            if (Test-Path -LiteralPath $src) { Copy-Item -LiteralPath $src -Destination (Join-Path $tmp ($dbName + $suffix)) }
        }
        $prevEap = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try { $out = & $sqlite.Source -readonly -separator ',' (Join-Path $tmp $dbName) $Query 2>&1 }
        finally { $ErrorActionPreference = $prevEap }
        if ($LASTEXITCODE -ne 0) { return @{ Status = 'sqlite3 kunne ikke læse tabellen (ældre appversion?)'; Lines = @() } }
        return @{ Status = 'ok'; Lines = @($out | ForEach-Object { "$_" }) }
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Get-HistoryLatencies([int]$Max) {
    $res = Invoke-DictationQuery "SELECT latency_ms, audio_ms FROM dictation_history WHERE insert_method IS NOT NULL ORDER BY created_at DESC LIMIT $Max;"
    $rows = @()
    foreach ($line in $res.Lines) {
        $parts = $line.Split(',')
        if ($parts.Count -eq 2) {
            $rows += [pscustomobject]@{ LatencyMs = [double]$parts[0]; AudioMs = [double]$parts[1] }
        }
    }
    return @{ Status = $res.Status; Rows = $rows }
}

# Tider pr. trin fra tabellen dictation_timings (kun tal, aldrig tekst). De seneste $Max pr. udbyder.
function Get-StepTimings([int]$Max) {
    $cols = 'provider, start_ms, capture_ms, transcribe_ms, polish_ms, insert_ms, total_ms, audio_ms'
    $res = Invoke-DictationQuery "SELECT $cols FROM dictation_timings WHERE inserted = 1 ORDER BY created_at DESC, id DESC LIMIT $($Max * 4);"
    $rows = @()
    foreach ($line in $res.Lines) {
        $p = $line.Split(',')
        if ($p.Count -ne 8) { continue }
        $num = { param($s) if ($s -eq '') { $null } else { [double]$s } }
        $rows += [pscustomobject]@{
            Provider = $p[0]; StartMs = (& $num $p[1]); CaptureMs = (& $num $p[2]); TranscribeMs = (& $num $p[3])
            PolishMs = (& $num $p[4]); InsertMs = (& $num $p[5]); TotalMs = (& $num $p[6]); AudioMs = (& $num $p[7])
        }
    }
    $byProvider = [ordered]@{}
    foreach ($name in @($rows | ForEach-Object { $_.Provider } | Sort-Object -Unique)) {
        $byProvider[$name] = @($rows | Where-Object { $_.Provider -eq $name } | Select-Object -First $Max)
    }
    return @{ Status = $res.Status; Providers = $byProvider }
}

function Get-StepValues($Rows, [string]$Prop) {
    return [double[]]@($Rows | ForEach-Object { $_.$Prop } | Where-Object { $null -ne $_ })
}

function Summarize-Latency($Rows) {
    $lat = @($Rows | ForEach-Object { $_.LatencyMs })
    # "15 s tale": optagelser mellem 10 og 20 s.
    $mid = @($Rows | Where-Object { $_.AudioMs -ge 10000 -and $_.AudioMs -le 20000 } | ForEach-Object { $_.LatencyMs })
    return @{
        N = $lat.Count
        P50 = Get-Percentile $lat 50
        P95 = Get-Percentile $lat 95
        NMid = $mid.Count
        P50Mid = Get-Percentile $mid 50
        P95Mid = Get-Percentile $mid 95
    }
}

# ---------------------------------------------------------------- maskineinfo

function Get-WebView2Version {
    $keys = @(
        'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
        'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
        'HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
    )
    foreach ($k in $keys) {
        $v = (Get-ItemProperty -Path $k -Name pv -ErrorAction SilentlyContinue).pv
        if ($v) { return $v }
    }
    return 'ikke fundet'
}

function Get-MachineInfo {
    $cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
    $os = Get-CimInstance Win32_OperatingSystem
    $cs = Get-CimInstance Win32_ComputerSystem
    $ubr = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction SilentlyContinue).UBR
    $build = $os.BuildNumber
    if ($ubr) { $build = "$build.$ubr" }
    $appVer = 'ukendt'
    if (Test-Path -LiteralPath $ExeSti) { $appVer = (Get-Item -LiteralPath $ExeSti).VersionInfo.ProductVersion }
    return [ordered]@{
        Dato      = (Get-Date).ToString('yyyy-MM-dd HH:mm')
        CPU       = ($cpu.Name -replace '\s+', ' ').Trim()
        Kerner    = "$($cpu.NumberOfCores) kerner / $($cpu.NumberOfLogicalProcessors) tråde"
        RAM       = "$([math]::Round($cs.TotalPhysicalMemory / 1GB, 1)) GB"
        Windows   = "$($os.Caption) build $build"
        WebView2  = Get-WebView2Version
        Appversion = $appVer
    }
}

# ================================================================ kørsel

if ($Repetitioner -lt 1) { throw 'Repetitioner skal være mindst 1.' }

if ($KunHukommelse) {
    if ((Get-AppProcesses).Count -eq 0) { Write-Warning 'tingskriver.exe kører ikke.'; exit 5 }
    $m = Measure-Idle 10
    Write-Output ''
    Write-Output '| Måling | Værdi | Enhed |'
    Write-Output '|---|---|---|'
    Write-Output ("| Working set, i alt | {0} | MB |" -f (Format-Num $m.TotalWS))
    Write-Output ("| Private bytes, i alt | {0} | MB |" -f (Format-Num $m.TotalPB))
    Write-Output ("| - tingskriver.exe, working set | {0} | MB |" -f (Format-Num $m.AppWS))
    Write-Output ("| - msedgewebview2.exe, working set ({1} processer) | {0} | MB |" -f (Format-Num $m.WebViewWS), $m.WebViewCount)
    Write-Output ("| - øvrige underprocesser, working set ({1} processer) | {0} | MB |" -f (Format-Num $m.AndreWS), $m.AndreCount)
    Write-Output ("| CPU (10 s) | {0} | % |" -f (Format-Num $m.CpuPct 2))
    exit 0
}

Assert-NoRecordingInProgress

$machine = Get-MachineInfo
$result = [ordered]@{}

if (-not $KunDiktering) {
    if (-not (Test-Path -LiteralPath $ExeSti)) { throw "Appen findes ikke: $ExeSti (brug -ExeSti)." }
    Assert-AppNotRunning
}

if ($KunKoldstart) {
    Write-Host 'Måler én koldstart (kør lige efter en genstart af pc''en for et rigtigt koldt tal).'
    $r = Start-AppTimed
    Stop-AppGracefully $r.Process
    if ($null -eq $r.Ms) { Write-Warning 'Hovedvinduet blev ikke synligt inden for 60 s.'; exit 4 }
    Write-Host ("Koldstart: {0} ms" -f (Format-Num $r.Ms))
    if ($KoldLog) {
        [IO.File]::AppendAllText($KoldLog, ([math]::Round($r.Ms).ToString([Globalization.CultureInfo]::InvariantCulture) + "`r`n"))
        $all = @(Get-Content -LiteralPath $KoldLog | Where-Object { $_ -match '^\d+(\.\d+)?$' } |
            ForEach-Object { [double]::Parse($_, [Globalization.CultureInfo]::InvariantCulture) })
        Write-Host ("Median over {0} kørsler i {1}: {2} ms" -f $all.Count, $KoldLog, (Format-Num (Get-Median $all)))
    }
    exit 0
}

if (-not $KunDiktering) {
    $koldMs = @(); $varmMs = @()
    $idle = @(); $late = @()
    for ($i = 1; $i -le $Repetitioner; $i++) {
        Write-Host "Kørsel $i af ${Repetitioner}: starter appen..."
        Assert-NoRecordingInProgress
        $r = Start-AppTimed
        if ($null -eq $r.Ms) {
            Write-Warning 'Hovedvinduet blev ikke synligt inden for 60 s (appen starter måske minimeret til tray). Tiden tæller ikke med.'
        } elseif ($i -eq 1) {
            $koldMs += $r.Ms
        } else {
            $varmMs += $r.Ms
        }
        Write-Host "  tomgangsmåling om $TomgangSekunder s..."
        $idle += , (Measure-Idle $TomgangSekunder)
        if ($EfterMinutter -gt 0) {
            # CloseMainWindow skjuler hovedvinduet i tray, så målingen efter ventetiden er en tray-måling.
            foreach ($p in (Get-AppProcesses)) { [void]$p.CloseMainWindow() }
            Write-Host "  vinduet skjult i tray; venter $EfterMinutter min før næste måling..."
            Start-Sleep -Seconds ($EfterMinutter * 60)
            $late += , (Measure-Idle 10)
        }
        Write-Host '  lukker appen...'
        Stop-AppGracefully $r.Process
        Write-Host '  pause 3 s'
        Start-Sleep -Seconds 3
    }
    $result.Kold = $koldMs
    $result.Varm = $varmMs
    $result.Idle = $idle
    $result.Late = $late
}

$logRows = @()
$histRes = @{ Status = 'sprunget over'; Rows = @() }
try { $logRows = Get-LogLatencies $Dikteringer } catch { Write-Warning "Kunne ikke læse loggen: $($_.Exception.Message)" }
try { $histRes = Get-HistoryLatencies $Dikteringer } catch { Write-Warning "Kunne ikke læse historikken: $($_.Exception.Message)" }
$stepRes = @{ Status = 'sprunget over'; Providers = [ordered]@{} }
try { $stepRes = Get-StepTimings $Dikteringer } catch { Write-Warning "Kunne ikke læse dikteringstider: $($_.Exception.Message)" }
$logSum = Summarize-Latency $logRows
$histSum = Summarize-Latency $histRes.Rows

# ---------------------------------------------------------------- rapport

function Med($Samples, [string]$Key) {
    if (-not $Samples -or $Samples.Count -eq 0) { return $null }
    return Get-Median ([double[]]@($Samples | ForEach-Object { $_[$Key] }))
}

$sb = New-Object System.Text.StringBuilder
[void]$sb.AppendLine('## Maskine')
[void]$sb.AppendLine()
[void]$sb.AppendLine('| Felt | Værdi |')
[void]$sb.AppendLine('|---|---|')
foreach ($k in $machine.Keys) { [void]$sb.AppendLine("| $k | $($machine[$k]) |") }
[void]$sb.AppendLine()
[void]$sb.AppendLine("## Resultater (median af $Repetitioner kørsler)")
[void]$sb.AppendLine()
[void]$sb.AppendLine('| Måling | Median | Enhed | Note |')
[void]$sb.AppendLine('|---|---|---|---|')

$json = [ordered]@{ maskine = $machine; repetitioner = $Repetitioner; resultater = [ordered]@{} }

function Add-Row([string]$Name, $Value, [string]$Unit, [string]$Note, [string]$JsonKey, [int]$Dec = 0) {
    [void]$sb.AppendLine("| $Name | $(Format-Num $Value $Dec) | $Unit | $Note |")
    if ($JsonKey) {
        $v = $null
        if ($null -ne $Value) { $v = [math]::Round([double]$Value, [math]::Max($Dec, 1)) }
        $json.resultater[$JsonKey] = $v
    }
}

if (-not $KunDiktering) {
    Add-Row 'Koldstart (første start i kørslen)' (Get-Median ([double[]]$result.Kold)) 'ms' "n = $($result.Kold.Count), ikke en median; proces start til synligt hovedvindue; ægte kold kræver -KunKoldstart efter genstart" 'koldstart_ms'
    Add-Row 'Varmstart' (Get-Median ([double[]]$result.Varm)) 'ms' "efter lukning, $($result.Varm.Count) målinger" 'varmstart_ms'
    $i0 = $result.Idle
    Add-Row "Tomgang efter $TomgangSekunder s: working set, i alt" (Med $i0 'TotalWS') 'MB' 'app + alle underprocesser' 'tomgang_ws_total_mb'
    Add-Row "Tomgang efter $TomgangSekunder s: private bytes, i alt" (Med $i0 'TotalPB') 'MB' 'app + alle underprocesser' 'tomgang_pb_total_mb'
    Add-Row '- tingskriver.exe: working set' (Med $i0 'AppWS') 'MB' '' 'tomgang_ws_app_mb'
    Add-Row '- tingskriver.exe: private bytes' (Med $i0 'AppPB') 'MB' '' 'tomgang_pb_app_mb'
    Add-Row '- msedgewebview2.exe: working set' (Med $i0 'WebViewWS') 'MB' "antal processer (median): $(Format-Num (Med $i0 'WebViewCount'))" 'tomgang_ws_webview_mb'
    Add-Row '- msedgewebview2.exe: private bytes' (Med $i0 'WebViewPB') 'MB' '' 'tomgang_pb_webview_mb'
    Add-Row '- øvrige underprocesser: working set' (Med $i0 'AndreWS') 'MB' "llama-helper, ffmpeg m.fl.; antal (median): $(Format-Num (Med $i0 'AndreCount'))" 'tomgang_ws_andre_mb'
    Add-Row '- øvrige underprocesser: private bytes' (Med $i0 'AndrePB') 'MB' '' 'tomgang_pb_andre_mb'
    Add-Row 'Tomgang: CPU' (Med $i0 'CpuPct') '%' 'gennemsnit over 10 s, af hele maskinen' 'tomgang_cpu_pct' 2
    if ($EfterMinutter -gt 0) {
        $l0 = $result.Late
        Add-Row "Efter $EfterMinutter min: working set, i alt" (Med $l0 'TotalWS') 'MB' 'app + alle underprocesser' 'efter_ws_total_mb'
        Add-Row "Efter $EfterMinutter min: private bytes, i alt" (Med $l0 'TotalPB') 'MB' 'app + alle underprocesser' 'efter_pb_total_mb'
        Add-Row "Efter $EfterMinutter min: CPU" (Med $l0 'CpuPct') '%' 'gennemsnit over 10 s' 'efter_cpu_pct' 2
    }
}

Add-Row 'Diktering p50 (log)' $logSum.P50 'ms' "slip til indsat, $($logSum.N) seneste" 'diktering_log_p50_ms'
Add-Row 'Diktering p95 (log)' $logSum.P95 'ms' "slip til indsat, $($logSum.N) seneste" 'diktering_log_p95_ms'
Add-Row 'Diktering p50, 10-20 s tale (log)' $logSum.P50Mid 'ms' "n = $($logSum.NMid); budgetmål 1500 ms" 'diktering_log_p50_15s_ms'
Add-Row 'Diktering p95, 10-20 s tale (log)' $logSum.P95Mid 'ms' "n = $($logSum.NMid)" 'diktering_log_p95_15s_ms'
Add-Row 'Diktering p50 (historik)' $histSum.P50 'ms' "n = $($histSum.N); $($histRes.Status)" 'diktering_hist_p50_ms'
Add-Row 'Diktering p95 (historik)' $histSum.P95 'ms' "n = $($histSum.N)" 'diktering_hist_p95_ms'

# Tider pr. trin og udbyder (tabellen dictation_timings, appen fra 0.5.1).
if ($stepRes.Providers.Count -eq 0) {
    [void]$sb.AppendLine("| Diktering pr. trin | n/a | | $($stepRes.Status); ingen dikteringer i dictation_timings endnu |")
}
foreach ($prov in $stepRes.Providers.Keys) {
    $r = $stepRes.Providers[$prov]
    $n = $r.Count
    $total = Get-StepValues $r 'TotalMs'
    Add-Row "Diktering $prov p50 (trin)" (Get-Median $total) 'ms' "slip til indsat, n = $n; advarsel over 1500" "diktering_${prov}_p50_ms"
    Add-Row "Diktering $prov p95 (trin)" (Get-Percentile $total 95) 'ms' "n = $n" "diktering_${prov}_p95_ms"
    $mid = [double[]]@($r | Where-Object { $_.AudioMs -ge 10000 -and $_.AudioMs -le 20000 } | ForEach-Object { $_.TotalMs })
    Add-Row "Diktering $prov p50, 10-20 s tale (trin)" (Get-Median $mid) 'ms' "n = $($mid.Count)" "diktering_${prov}_p50_15s_ms"
    foreach ($step in @(
            @('StartMs', 'tastetryk til optagelse', 'start'),
            @('CaptureMs', 'slip til lyd klar', 'capture'),
            @('TranscribeMs', 'transskription', 'transcribe'),
            @('PolishMs', 'polering', 'polish'),
            @('InsertMs', 'indsættelse', 'insert'))) {
        $v = Get-StepValues $r $step[0]
        if ($v.Count -eq 0) { continue }
        Add-Row "- $prov $($step[1]) p50 / p95" (Get-Median $v) 'ms' "p95 $(Format-Num (Get-Percentile $v 95)) ms, n = $($v.Count)" "diktering_${prov}_$($step[2])_p50_ms"
    }
}

Write-Output ''
Write-Output $sb.ToString()
Write-Output '## Maskinlæsbart'
Write-Output ''
Write-Output '```json'
Write-Output ($json | ConvertTo-Json -Depth 5)
Write-Output '```'
Write-Output ''
Write-Output 'Ikke målt af scriptet: optagelse af to spor, hukommelse efter et 30 min møde og efter en time i tray (kræver at du selv optager eller venter; brug -EfterMinutter for tray).'

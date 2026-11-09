<#
.SYNOPSIS
  E14 budget check: runs the release exe and samples private memory and CPU once per second.
.DESCRIPTION
  Uses a temporary %APPDATA% (the user's config is untouched). The app shows either the HUD or
  the mini bar, never both: -Window picks which one is visible (default Hud, the heavier one).
  Budget (constitution): private memory <= 30 MB and average CPU <= 1%.
  The constitution does not say 1% of what, so the gate uses the stricter reading,
  % of ONE core (= CPU time / wall time). % of the whole machine (that / logical cores)
  is printed too and is always lower.
  -Scanner opens and closes the scanner window 5 times (E14 second scenario, through the
  app's --scanner-cycles flag) and checks private memory is <= 30 MB within 10 s afterwards.
#>
param(
    [string]$Exe = (Join-Path $(if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $PSScriptRoot '..\target' }) 'release\hud-app.exe'),
    [int]$DurationSeconds = 600,
    [int]$WarmupSeconds = 15,
    [ValidateSet('Hud', 'MiniBar')][string]$Window = 'Hud',
    [switch]$Scanner,
    [double]$MaxMemoryMb = 30,
    [double]$MaxCpuPercent = 1
)
$ErrorActionPreference = 'Stop'
if (-not (Test-Path $Exe)) { throw "exe not found: $Exe (cargo build --release)" }

$appData = Join-Path ([IO.Path]::GetTempPath()) ("hud-measure-" + [guid]::NewGuid())
$dir = Join-Path $appData 'hud-monitor'
New-Item -ItemType Directory -Force $dir | Out-Null
$hudOn = if ($Window -eq 'Hud') { 'true' } else { 'false' }
$barOn = if ($Window -eq 'MiniBar') { 'true' } else { 'false' }
@"
profiles = []
autostart = false

[hud]
x = 100
y = 100
opacity = 0.88
visible = $hudOn
acrylic = false
click_through = false

[minibar]
items = ["Cpu", "Ram", "Net", "Temp"]
orientation = "Horizontal"

[minibar.window]
x = 100
y = 100
opacity = 0.88
visible = $barOn
acrylic = false
click_through = false

[thresholds.cpu_percent]
warn = 70.0
crit = 90.0

[thresholds.ram_percent]
warn = 75.0
crit = 90.0

[thresholds.temp_c]
warn = 75.0
crit = 90.0

[thresholds.ping_ms]
warn = 80.0
crit = 200.0

[thresholds.claude_5h]
warn = 70.0
crit = 90.0

[hotkeys]
toggle_hud = "Ctrl+Alt+H"
toggle_minibar = "Ctrl+Alt+M"
"@ | Set-Content -Encoding ascii (Join-Path $dir 'config.toml')

$cores = [Environment]::ProcessorCount
$oldAppData = $env:APPDATA
$env:APPDATA = $appData
if ($Scanner) { $proc = Start-Process $Exe -ArgumentList '--scanner-cycles', '5' -PassThru }
else { $proc = Start-Process $Exe -PassThru }
$env:APPDATA = $oldAppData

function Get-Mb($p) { $p.Refresh(); $p.PrivateMemorySize64 / 1MB }

$failed = $false
try {
    Start-Sleep -Seconds 3
    if ($proc.HasExited) { throw "app exited early with code $($proc.ExitCode)" }

    if ($Scanner) {
        Start-Sleep -Seconds 12   # 5 open/close cycles, 2 s each
        $deadline = (Get-Date).AddSeconds(10)
        do {
            $mb = Get-Mb $proc
            if ($mb -le $MaxMemoryMb) { break }
            Start-Sleep -Seconds 1
        } while ((Get-Date) -lt $deadline)
        $mb = Get-Mb $proc
        "Scanner x5: private memory {0:N1} MB within 10 s of the last close (budget <= {1} MB)" -f $mb, $MaxMemoryMb
        if ($mb -gt $MaxMemoryMb) { $failed = $true; 'FAIL: memory did not return under budget' }
    }
    else {
        Start-Sleep -Seconds $WarmupSeconds
        $proc.Refresh()
        $cpu0 = $proc.TotalProcessorTime.TotalSeconds
        $t0 = [Diagnostics.Stopwatch]::StartNew()
        $maxMb = 0.0
        $samples = 0
        while ($t0.Elapsed.TotalSeconds -lt $DurationSeconds) {
            Start-Sleep -Seconds 1
            if ($proc.HasExited) { throw 'app exited during measurement' }
            $maxMb = [Math]::Max($maxMb, (Get-Mb $proc))
            $samples++
        }
        $proc.Refresh()
        $cpuSeconds = $proc.TotalProcessorTime.TotalSeconds - $cpu0
        $wall = $t0.Elapsed.TotalSeconds
        $oneCore = 100 * $cpuSeconds / $wall
        $machine = $oneCore / $cores
        "Window: $Window, $samples samples over {0:N0} s, $cores logical cores" -f $wall
        "Max private memory: {0:N1} MB (budget <= {1} MB)" -f $maxMb, $MaxMemoryMb
        "Average CPU: {0:N2}% of one core (budget gate, <= {1}%) | {2:N2}% of the whole machine" -f $oneCore, $MaxCpuPercent, $machine
        if ($maxMb -gt $MaxMemoryMb) { $failed = $true; 'FAIL: memory over budget' }
        if ($oneCore -gt $MaxCpuPercent) { $failed = $true; 'FAIL: CPU over budget' }
    }
}
finally {
    if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
    Remove-Item -Recurse -Force $appData -ErrorAction SilentlyContinue
}
if ($failed) { exit 1 }
'OK: within budget'

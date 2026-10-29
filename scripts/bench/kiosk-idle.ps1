<#
.SYNOPSIS
  Kiosk idle benchmark (release, reference hardware): with the Kiosk installed, starts the Kiosk
  app, lets it idle with no player session and checks the Epic 1 budgets.

.DESCRIPTION
  - Expects the installer to have registered and started the arena-kiosk-svc service, and the
    reference machine's console to be signed in as the player account (autologon): the app
    runs the way it does in production, launched by the service's watchdog into the player's
    session, never by this script.
  - Stops any running app, then waits (up to -ConnectSeconds) for the service to log
    "app connected" in its ring under -DataDir\logs: the app is up and on the pipe. Then idles
    for -IdleSeconds (default 5 minutes).
  - Sums working-set memory of the service, the app and the app's WebView2 child processes:
    together they must stay under -LimitMB (80 MB).
  - Reads the "GPU Engine" utilisation counters and fails if any kiosk process uses the GPU
    above 0 %.
  -Phase attract (default) measures the kiosk idle on the attract screen. -Phase shell measures it
  idle in the player shell (Story 6.38): the caller has started a session on this PC (for
  example the 2-minute test session of the setup wizard, or a Hub-side start) and the app is
  left running as it is, so the shell stays open. -OutJson writes {phase, mb} for
  scripts/journey-report.mjs.
  Exits 0 when every budget holds and 1 otherwise. The service and its app are left running
  (the uninstall step removes them).

.EXAMPLE
  scripts/bench/kiosk-idle.ps1 -UiExe "$env:ProgramFiles\Arena 360 Kiosk\arena-kiosk-ui.exe"
#>
param(
  [Parameter(Mandatory = $true)] [string] $UiExe,
  [string] $DataDir = (Join-Path $env:ProgramData "Arena360\Kiosk"),
  [int] $ConnectSeconds = 180,
  [int] $IdleSeconds = 300,
  [int] $LimitMB = 80,
  [int] $GpuSamples = 10,
  [ValidateSet("attract", "shell")] [string] $Phase = "attract",
  [string] $OutJson = ""
)

$ErrorActionPreference = "Stop"
$SvcName = "arena-kiosk-svc"
$UiName = [System.IO.Path]::GetFileNameWithoutExtension($UiExe)

# The newest "app connected" line in the service's log ring after $Since, or $null.
function Find-AppConnected([datetime] $Since) {
  $logs = Join-Path $DataDir "logs"
  $files = Get-ChildItem $logs -Filter "kiosk-*.jsonl" -ErrorAction SilentlyContinue
  foreach ($f in $files) {
    foreach ($line in Get-Content $f.FullName -Tail 200 -ErrorAction SilentlyContinue) {
      if ($line -notmatch '"message":"app connected"') { continue }
      try { $at = [datetime]::Parse(($line | ConvertFrom-Json).timestamp).ToUniversalTime() } catch { continue }
      if ($at -gt $Since) { return $at }
    }
  }
  return $null
}

# Every process below $RootId (WebView2 browser, renderer, GPU and utility processes).
function Get-Descendants([int] $RootId) {
  $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name
  $found = New-Object System.Collections.Generic.List[object]
  $queue = New-Object System.Collections.Generic.Queue[int]
  $queue.Enqueue($RootId)
  while ($queue.Count -gt 0) {
    $parent = $queue.Dequeue()
    foreach ($p in $all | Where-Object { $_.ParentProcessId -eq $parent }) {
      $found.Add($p)
      $queue.Enqueue([int] $p.ProcessId)
    }
  }
  return $found
}

function Get-WorkingSetMB([int[]] $Ids) {
  $bytes = 0
  foreach ($id in $Ids) {
    $p = Get-Process -Id $id -ErrorAction SilentlyContinue
    if ($p) { $bytes += $p.WorkingSet64 }
  }
  return [math]::Round($bytes / 1MB, 1)
}

if (-not (Test-Path $UiExe)) { throw "Kiosk app not found: $UiExe" }
$svc = Get-Service -Name $SvcName -ErrorAction Stop
if ($svc.Status -ne "Running") { Start-Service -Name $SvcName }

$since = (Get-Date).ToUniversalTime()
# The service's watchdog relaunches it in the player's session. In the shell phase the app is
# left alone: restarting it would drop the player's session screen.
if ($Phase -eq "attract") {
  Get-Process -Name $UiName -ErrorAction SilentlyContinue | Stop-Process -Force
}

$failures = New-Object System.Collections.Generic.List[string]
& {
  $deadline = (Get-Date).AddSeconds($ConnectSeconds)
  while ($Phase -eq "attract" -and $null -eq (Find-AppConnected $since)) {
    if ((Get-Date) -gt $deadline) {
      throw "the service never logged 'app connected' (is the console signed in as the player?)"
    }
    Start-Sleep -Seconds 1
  }
  Write-Host "Kiosk app is up ($Phase); idling for $IdleSeconds s"
  Start-Sleep -Seconds $IdleSeconds

  $svcProc = @(Get-Process -Name $SvcName -ErrorAction SilentlyContinue)
  if ($svcProc.Count -ne 1) { throw "expected one $SvcName process, found $($svcProc.Count)" }
  # The service may have relaunched the app: measure whichever app process is live now.
  $apps = @(Get-Process -Name $UiName -ErrorAction SilentlyContinue)
  if ($apps.Count -ne 1) { throw "expected one $UiName process, found $($apps.Count)" }
  $tree = @([int] $svcProc[0].Id, [int] $apps[0].Id) + @(Get-Descendants $apps[0].Id |
      ForEach-Object { [int] $_.ProcessId })
  $mb = Get-WorkingSetMB $tree
  Write-Host ("service + app + WebView2 ({0} processes): {1} MB (limit {2} MB)" -f $tree.Count, $mb, $LimitMB)
  if ($mb -ge $LimitMB) { $failures.Add("the kiosk uses $mb MB (limit $LimitMB MB)") }
  if ($OutJson) {
    $dir = Split-Path -Parent $OutJson
    if ($dir) { New-Item -ItemType Directory -Force $dir | Out-Null }
    (@{ phase = $Phase; mb = $mb } | ConvertTo-Json -Compress) | Set-Content -Path $OutJson -Encoding ascii
  }

  $samples = $null
  try {
    $samples = (Get-Counter -Counter "\GPU Engine(*)\Utilization Percentage" `
        -SampleInterval 1 -MaxSamples $GpuSamples -ErrorAction Stop).CounterSamples
  } catch {
    $failures.Add("GPU counters unavailable: $($_.Exception.Message)")
  }
  if ($null -ne $samples) {
    $busy = $samples | Where-Object {
      $_.InstanceName -match "pid_(\d+)_" -and $tree -contains [int] $Matches[1] -and $_.CookedValue -gt 0
    }
    $gpuPeak = ($busy | Measure-Object -Property CookedValue -Maximum).Maximum
    if ($busy) {
      $names = ($busy | ForEach-Object { $_.InstanceName } | Sort-Object -Unique) -join ", "
      $failures.Add("GPU in use by a kiosk process (peak $gpuPeak %): $names")
    } else {
      Write-Host "GPU: no kiosk process used any engine over $GpuSamples s"
    }
  }
}

if ($failures.Count -gt 0) {
  foreach ($f in $failures) { Write-Host "FAIL: $f" }
  exit 1
}
Write-Host "kiosk-idle: every budget holds"
exit 0

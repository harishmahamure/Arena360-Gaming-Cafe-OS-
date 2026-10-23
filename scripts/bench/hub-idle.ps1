<#
.SYNOPSIS
  Hub idle benchmark (release, reference hardware): starts the installed Hub UI, waits for it
  to go idle and checks the Epic 1 budgets.

.DESCRIPTION
  - Starts the Hub UI, which probes 127.0.0.1:7409/healthz and spawns its arena-hub sidecar.
  - Waits until /healthz answers, then idles for -IdleSeconds (default 5 minutes).
  - Sums working-set memory:
      * the arena-hub service process must stay under -HubLimitMB (50 MB);
      * the Hub UI plus its WebView2 child processes must stay under -UiLimitMB (250 MB).
  - Reads the "GPU Engine" utilisation counters and fails if any Hub process uses the GPU
    above 0 %.
  Exits 0 when every budget holds and 1 otherwise. The processes it started are stopped.

.EXAMPLE
  scripts/bench/hub-idle.ps1 -UiExe "$env:LOCALAPPDATA\Arena 360 Hub\arena-hub-ui.exe" -DataDir C:\bench\hub
#>
param(
  [Parameter(Mandatory = $true)] [string] $UiExe,
  # Data dir for the sidecar (inherited through ARENA_HUB_DATA_DIR); empty keeps its default.
  [string] $DataDir = "",
  [int] $IdleSeconds = 300,
  [int] $HubLimitMB = 50,
  [int] $UiLimitMB = 250,
  [int] $GpuSamples = 10
)

$ErrorActionPreference = "Stop"
$HubName = "arena-hub"
$UiName = [System.IO.Path]::GetFileNameWithoutExtension($UiExe)

function Stop-Hub {
  Get-Process -Name $UiName, $HubName -ErrorAction SilentlyContinue | Stop-Process -Force
}

function Test-Healthz {
  try {
    $r = Invoke-WebRequest -UseBasicParsing -TimeoutSec 2 -Uri "http://127.0.0.1:7409/healthz"
    return $r.StatusCode -eq 200
  } catch {
    return $false
  }
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

if (-not (Test-Path $UiExe)) { throw "Hub UI not found: $UiExe" }

# Start from nothing so the UI spawns its own sidecar.
Stop-Hub
Start-Sleep -Seconds 2
if ($DataDir) {
  New-Item -ItemType Directory -Force $DataDir | Out-Null
  $env:ARENA_HUB_DATA_DIR = $DataDir
}

$failures = New-Object System.Collections.Generic.List[string]
$ui = Start-Process -FilePath $UiExe -PassThru
try {
  $deadline = (Get-Date).AddSeconds(60)
  while (-not (Test-Healthz)) {
    if ((Get-Date) -gt $deadline) { throw "the Hub service never answered /healthz" }
    Start-Sleep -Milliseconds 500
  }
  Write-Host "Hub is up; idling for $IdleSeconds s"
  Start-Sleep -Seconds $IdleSeconds

  $hub = @(Get-Process -Name $HubName -ErrorAction SilentlyContinue)
  if ($hub.Count -ne 1) { throw "expected one $HubName process, found $($hub.Count)" }
  # The sidecar is spawned by the UI, so leave it out of the UI tree.
  $uiTree = @([int] $ui.Id) + @(Get-Descendants $ui.Id |
      Where-Object { $_.Name -ne "$HubName.exe" } |
      ForEach-Object { [int] $_.ProcessId })
  $hubMB = Get-WorkingSetMB @([int] $hub[0].Id)
  $uiMB = Get-WorkingSetMB $uiTree

  Write-Host ("{0} working set: {1} MB (limit {2} MB)" -f $HubName, $hubMB, $HubLimitMB)
  Write-Host ("Hub UI + WebView2 ({0} processes): {1} MB (limit {2} MB)" -f $uiTree.Count, $uiMB, $UiLimitMB)
  if ($hubMB -ge $HubLimitMB) { $failures.Add("$HubName uses $hubMB MB (limit $HubLimitMB MB)") }
  if ($uiMB -ge $UiLimitMB) { $failures.Add("the Hub UI uses $uiMB MB (limit $UiLimitMB MB)") }

  # GPU: any engine utilisation above 0 for a Hub process fails.
  $hubPids = @([int] $hub[0].Id) + $uiTree
  $samples = $null
  try {
    $samples = (Get-Counter -Counter "\GPU Engine(*)\Utilization Percentage" `
        -SampleInterval 1 -MaxSamples $GpuSamples -ErrorAction Stop).CounterSamples
  } catch {
    $failures.Add("GPU counters unavailable: $($_.Exception.Message)")
  }
  if ($null -ne $samples) {
    $busy = $samples | Where-Object {
      $_.InstanceName -match "pid_(\d+)_" -and $hubPids -contains [int] $Matches[1] -and $_.CookedValue -gt 0
    }
    $gpuPeak = ($busy | Measure-Object -Property CookedValue -Maximum).Maximum
    if ($busy) {
      $names = ($busy | ForEach-Object { $_.InstanceName } | Sort-Object -Unique) -join ", "
      $failures.Add("GPU in use by a Hub process (peak $gpuPeak %): $names")
    } else {
      Write-Host "GPU: no Hub process used any engine over $GpuSamples s"
    }
  }
} finally {
  Stop-Hub
}

if ($failures.Count -gt 0) {
  foreach ($f in $failures) { Write-Host "FAIL: $f" }
  exit 1
}
Write-Host "hub-idle: every budget holds"
exit 0

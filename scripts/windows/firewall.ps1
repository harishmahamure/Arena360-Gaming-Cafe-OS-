<#
.SYNOPSIS
  Opens (or removes) the Arena 360 Hub's LAN ports in Windows Defender Firewall.

.DESCRIPTION
  Adds inbound allow rules for TCP 7400 (paired apps, TLS WebSocket) and TCP 7401 (TLS for
  paired apps, plain HTTP/WebSocket for kitchen screens and phones) on the Private profile
  only. No other inbound port is opened: the Hub's health port stays on loopback.

  The Hub installer story calls this script; CI does not run it. Run it elevated.

.PARAMETER Remove
  Removes the rules this script added instead of adding them.

.PARAMETER LanPort
  The paired-app TLS port (default 7400; matches the Hub's --lan-port).

.PARAMETER WebPort
  The sniffed TLS / HTTP port (default 7401; matches the Hub's --web-port).

.PARAMETER Program
  Optional path to arena-hub.exe. When given, the rules apply to that program only.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\windows\firewall.ps1
.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\windows\firewall.ps1 -Remove
#>
[CmdletBinding()]
param(
  [switch]$Remove,
  [ValidateRange(1, 65535)][int]$LanPort = 7400,
  [ValidateRange(1, 65535)][int]$WebPort = 7401,
  [string]$Program
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$Group = 'Arena 360 Hub'
if ($LanPort -eq $WebPort) {
  Write-Error '-LanPort and -WebPort must differ.'
  exit 1
}
# Rule names are fixed so -Remove finds them whatever ports they were added with.
$Rules = @(
  @{ Name = 'Arena360-Hub-LAN'; Port = $LanPort; Description = 'Arena 360 Hub: paired apps (TLS WebSocket)' },
  @{ Name = 'Arena360-Hub-Web'; Port = $WebPort; Description = 'Arena 360 Hub: kitchen screens and phones (HTTP/WebSocket, TLS for paired apps)' }
)

$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  Write-Error 'firewall.ps1 must run elevated (as Administrator).'
  exit 1
}

if (-not $Remove) {
  # The rules only apply on Private networks: warn when the active network is not one.
  $profiles = @(Get-NetConnectionProfile -ErrorAction SilentlyContinue)
  $notPrivate = @($profiles | Where-Object { $_.NetworkCategory -ne 'Private' })
  if ($profiles.Count -eq 0 -or $notPrivate.Count -gt 0) {
    $names = ($notPrivate | ForEach-Object { "$($_.Name) ($($_.NetworkCategory))" }) -join ', '
    if (-not $names) { $names = 'no active network' }
    Write-Warning "The active network profile is not Private: $names. Kiosks and kitchen screens cannot reach the Hub until the cafe network is set to Private."
  }
}

foreach ($rule in $Rules) {
  $existing = Get-NetFirewallRule -Name $rule.Name -ErrorAction SilentlyContinue
  if ($existing) {
    Remove-NetFirewallRule -Name $rule.Name
  }
  if ($Remove) {
    Write-Output "removed $($rule.Name)"
    continue
  }
  $params = @{
    Name        = $rule.Name
    DisplayName = $rule.Name
    Description = $rule.Description
    Group       = $Group
    Direction   = 'Inbound'
    Action      = 'Allow'
    Protocol    = 'TCP'
    LocalPort   = $rule.Port
    Profile     = 'Private'
    Enabled     = 'True'
  }
  if ($Program) {
    $params.Program = $Program
  }
  New-NetFirewallRule @params | Out-Null
  Write-Output "added $($rule.Name) (TCP $($rule.Port), Private profile only)"
}

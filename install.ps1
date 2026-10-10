# ApexNorth installer for Windows. Paste this into PowerShell:
#
#   irm https://raw.githubusercontent.com/ElevenNorthCo/ApexNorth/main/install.ps1 | iex
#
# It downloads the latest release from GitHub, installs it for your user
# (no admin needed), and opens it. Run it again any time to update.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'  # the progress bar makes downloads very slow

$repo = 'ElevenNorthCo/ApexNorth'
$base = if ($env:APEXNORTH_VERSION) { "https://github.com/$repo/releases/download/$($env:APEXNORTH_VERSION)" } else { "https://github.com/$repo/releases/latest/download" }
$url = "$base/ApexNorth-Windows-Setup.exe"
$setup = Join-Path $env:TEMP 'ApexNorth-Setup.exe'

Write-Host '==> Downloading ApexNorth' -ForegroundColor Green
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
Invoke-WebRequest -Uri $url -OutFile $setup -UseBasicParsing

Write-Host '==> Installing' -ForegroundColor Green
Get-Process ApexNorth -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Process -FilePath $setup -ArgumentList '/S' -Wait
Remove-Item $setup -ErrorAction SilentlyContinue

$exe = @(
  (Join-Path $env:LOCALAPPDATA 'ApexNorth\ApexNorth.exe'),
  (Join-Path $env:LOCALAPPDATA 'Programs\ApexNorth\ApexNorth.exe'),
  (Join-Path $env:ProgramFiles 'ApexNorth\ApexNorth.exe')
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if ($exe) {
  Write-Host "==> Installed. Opening ApexNorth." -ForegroundColor Green
  Write-Host '    When Windows Firewall asks, allow ApexNorth on private networks so it can hear your PlayStation.'
  Start-Process $exe
} else {
  Write-Host '==> Installed. Find ApexNorth in the Start menu.' -ForegroundColor Green
}

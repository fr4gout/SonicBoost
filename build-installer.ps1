<#
  SonicBoost - build the Windows installer (.exe setup + .msi)

  Usage (PowerShell, in this folder):
      .\build-installer.ps1                 # build
      .\build-installer.ps1 -InstallTools   # first install Rust, Node.js and C++ build tools with winget, then build

  Output: .\release\SonicBoost_1.0.0_x64-setup.exe  and  .\release\SonicBoost_1.0.0_x64_en-US.msi
#>
param([switch]$InstallTools)

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

function Has($cmd) { [bool](Get-Command $cmd -ErrorAction SilentlyContinue) }
function Step($msg) { Write-Host "`n==> $msg" -ForegroundColor White }

if ($InstallTools) {
    if (-not (Has winget)) { throw "winget not found. Install 'App Installer' from the Microsoft Store first." }
    Step "Installing Microsoft C++ Build Tools (needed by Rust on Windows)..."
    winget install --id Microsoft.VisualStudio.2022.BuildTools -e --accept-package-agreements --accept-source-agreements `
        --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
    Step "Installing Rust..."
    winget install --id Rustlang.Rustup -e --accept-package-agreements --accept-source-agreements
    Step "Installing Node.js LTS..."
    winget install --id OpenJS.NodeJS.LTS -e --accept-package-agreements --accept-source-agreements
    $env:Path = [Environment]::GetEnvironmentVariable("Path", "Machine") + ";" + [Environment]::GetEnvironmentVariable("Path", "User")
    if (Has rustup) { rustup default stable-msvc | Out-Null }
}

Step "Checking tools"
$missing = @()
foreach ($c in "node", "npm", "cargo", "rustc") { if (-not (Has $c)) { $missing += $c } }
if ($missing.Count) {
    Write-Host "Missing: $($missing -join ', ')" -ForegroundColor Yellow
    Write-Host "Run:  .\build-installer.ps1 -InstallTools   (or see README.md -> Developer requirements)" -ForegroundColor Yellow
    exit 1
}
Write-Host ("node {0} | {1}" -f (node -v), (rustc --version))

Step "Installing npm packages (Tauri CLI)"
npm install --no-fund --no-audit

Step "Building SonicBoost (first build downloads crates and takes 5-10 min)"
npx tauri build
if ($LASTEXITCODE -ne 0) { throw "Build failed - see the messages above." }

Step "Collecting installers"
New-Item -ItemType Directory -Force -Path release | Out-Null
$bundle = "src-tauri\target\release\bundle"
Get-ChildItem "$bundle\nsis\*.exe", "$bundle\msi\*.msi" -ErrorAction SilentlyContinue | Copy-Item -Destination release -Force
Get-ChildItem release | ForEach-Object { Write-Host ("  {0}  ({1:N1} MB)" -f $_.Name, ($_.Length / 1MB)) -ForegroundColor Green }
Write-Host "`nDone. Run the -setup.exe in .\release to install SonicBoost." -ForegroundColor Green

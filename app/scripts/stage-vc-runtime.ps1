$ErrorActionPreference = "Stop"

$appRoot = Split-Path -Parent $PSScriptRoot
$runtime = Join-Path $appRoot "src-tauri/runtime/windows-x64"
if (-not (Test-Path $runtime)) {
    throw "Windows runtime directory not found: $runtime (run scripts/fetch-runtime.py first)"
}

$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio/Installer/vswhere.exe"
if (-not (Test-Path $vswhere)) {
    throw "vswhere.exe not found. Install the Visual Studio C++ build tools."
}

$vs = & $vswhere -latest -products * `
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
    -property installationPath
if (-not $vs) {
    throw "Visual Studio with the C++ x64 tools was not found."
}

$crt = Get-ChildItem (Join-Path $vs "VC/Redist/MSVC") -Directory |
    ForEach-Object {
        Get-ChildItem (Join-Path $_.FullName "x64") -Directory -Filter "Microsoft.VC14*.CRT" -ErrorAction SilentlyContinue
    } |
    Sort-Object FullName -Descending |
    Select-Object -First 1
if (-not $crt) {
    throw "The Visual C++ x64 redistributable CRT directory was not found under $vs."
}

Copy-Item (Join-Path $crt.FullName "*.dll") $runtime -Force
$license = Get-ChildItem $crt.FullName -Filter "LICENSE*" |
    Sort-Object Name -Descending |
    Select-Object -First 1
if ($license) {
    Copy-Item $license.FullName (Join-Path $runtime "VC-RUNTIME-LICENSE.txt") -Force
}

Write-Host "Staged Visual C++ runtime from: $($crt.FullName)"
Get-Item (Join-Path $runtime "msvcp140.dll"), (Join-Path $runtime "vcruntime140.dll"), (Join-Path $runtime "vcruntime140_1.dll") |
    ForEach-Object { Write-Host ("  {0} {1}" -f $_.Name, $_.VersionInfo.FileVersion) }

# Empaqueta el binario de release de Baud en un zip portable para Windows.
# Uso: pwsh ./tools/packaging/build_windows_portable.ps1

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $ScriptDir '..\..')
$DistDir = Join-Path $RepoRoot 'dist'
$Binary = Join-Path $RepoRoot 'target\release\baud.exe'
$LicenseSrc = Join-Path $RepoRoot 'LICENSE'
$CargoToml = Join-Path $RepoRoot 'Cargo.toml'

if (-not (Test-Path $Binary)) {
    Write-Error "Release binary not found at $Binary. Run 'cargo build --release' first."
}

if (-not (Test-Path $LicenseSrc)) {
    Write-Error "LICENSE not found at $LicenseSrc."
}

$versionMatch = Select-String -Path $CargoToml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
if (-not $versionMatch) {
    Write-Error "Could not read version from $CargoToml."
}
$Version = $versionMatch.Matches[0].Groups[1].Value

$PackageName = "baud-$Version-windows-x64"
$ZipPath = Join-Path $DistDir "$PackageName.zip"

$Staging = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
$PackageDir = Join-Path $Staging $PackageName
New-Item -ItemType Directory -Path $PackageDir -Force | Out-Null

try {
    Copy-Item $Binary (Join-Path $PackageDir 'baud.exe')
    Copy-Item $LicenseSrc (Join-Path $PackageDir 'LICENSE')

    $readme = @"
Baud $Version - Windows portable build
=======================================

Extract this folder anywhere and run baud.exe - no installer required.

Requirements: Windows 10 1809+ or Windows 11, with a DX12-capable GPU driver.

Docs: https://github.com/KarloZ7715/Baud/blob/master/docs/packaging/windows.md
"@
    Set-Content -Path (Join-Path $PackageDir 'README.txt') -Value $readme -Encoding UTF8

    $dumpbin = Get-Command 'dumpbin.exe' -ErrorAction SilentlyContinue
    if ($dumpbin) {
        $deps = & $dumpbin.Source /dependents (Join-Path $PackageDir 'baud.exe') 2>&1 | Out-String
        if ($deps -match 'VCRUNTIME140') {
            Write-Error "baud.exe depends on VCRUNTIME140 - static CRT (+crt-static) did not take effect."
        }
    }
    else {
        Write-Warning "dumpbin.exe not found on PATH - skipping VCRUNTIME140 dependency check."
    }

    New-Item -ItemType Directory -Path $DistDir -Force | Out-Null
    Compress-Archive -Path $PackageDir -DestinationPath $ZipPath -Force

    Write-Host "Portable zip created: $ZipPath"
}
finally {
    Remove-Item -Path $Staging -Recurse -Force -ErrorAction SilentlyContinue
}

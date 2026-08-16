# Construye el instalador MSI de Baud con WiX Toolset v4.
# Prerrequisitos: `dotnet tool install --global wix --version 4.0.5`
#                  `wix extension add WixToolset.UI.wixext/4.0.5`
# Uso: pwsh ./tools/packaging/build_windows_msi.ps1

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $ScriptDir '..\..')
$DistDir = Join-Path $RepoRoot 'dist'
$WixSource = Join-Path $RepoRoot 'packaging\windows\wix\baud.wxs'
$BinaryPath = Join-Path $RepoRoot 'target\release\baud.exe'
$LicensePath = Join-Path $RepoRoot 'LICENSE'
$ThirdPartyPath = Join-Path $RepoRoot 'LICENSES-THIRD-PARTY'
$CargoToml = Join-Path $RepoRoot 'Cargo.toml'
$BundleDir = Join-Path $RepoRoot 'target\conpty-bundle'
$FetchScript = Join-Path $ScriptDir 'fetch_conpty_bundle.ps1'
$IconPath = Join-Path $RepoRoot 'packaging\windows\baud.ico'
$LicenseRtfPath = Join-Path $RepoRoot 'packaging\windows\wix\license.rtf'

if (-not (Get-Command 'wix' -ErrorAction SilentlyContinue)) {
    Write-Error "wix CLI not found on PATH. Install with: dotnet tool install --global wix --version 4.0.5"
}

if (-not (Test-Path $BinaryPath)) {
    Write-Error "Release binary not found at $BinaryPath. Run 'cargo build --release' first."
}

if (-not (Test-Path $ThirdPartyPath)) {
    Write-Error "LICENSES-THIRD-PARTY not found at $ThirdPartyPath."
}

& $FetchScript -OutDir $BundleDir
$ConptyDllPath = Join-Path $BundleDir 'conpty.dll'
$OpenConsolePath = Join-Path $BundleDir 'OpenConsole.exe'
if (-not (Test-Path $ConptyDllPath) -or -not (Test-Path $OpenConsolePath)) {
    Write-Error "ConPTY bundle missing after fetch ($ConptyDllPath / $OpenConsolePath)."
}

$versionMatch = Select-String -Path $CargoToml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
if (-not $versionMatch) {
    Write-Error "Could not read version from $CargoToml."
}
$Version = $versionMatch.Matches[0].Groups[1].Value

# MSI ProductVersion solo admite 4 campos numericos.
$MsiVersion = if ($Version -match '^\d+\.\d+\.\d+$') { "$Version.0" } else { $Version }

New-Item -ItemType Directory -Path $DistDir -Force | Out-Null
$MsiPath = Join-Path $DistDir "baud-$Version-windows-x64.msi"

& wix build $WixSource `
    -arch x64 `
    -ext WixToolset.UI.wixext `
    -d "BaudVersion=$MsiVersion" `
    -d "BaudExePath=$BinaryPath" `
    -d "BaudLicensePath=$LicensePath" `
    -d "BaudThirdPartyLicensePath=$ThirdPartyPath" `
    -d "BaudConptyDllPath=$ConptyDllPath" `
    -d "BaudOpenConsolePath=$OpenConsolePath" `
    -d "BaudIconPath=$IconPath" `
    -d "BaudLicenseRtfPath=$LicenseRtfPath" `
    -out $MsiPath

if ($LASTEXITCODE -ne 0) {
    Write-Error "wix build failed with exit code $LASTEXITCODE"
}

Write-Host "MSI created: $MsiPath"

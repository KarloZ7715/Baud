# Descarga el NuGet pinneado de ConPTY, verifica SHA-256 y extrae el par x64.
# Uso: pwsh ./tools/packaging/fetch_conpty_bundle.ps1 -OutDir target/conpty-bundle

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$OutDir
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Version = '1.24.260710001'
$ExpectedSha256 = '175640566A3B59C4B132070EE96C2C77E5AB7EDD2E92732A5EB3610BBF63D90E'
$Url = "https://api.nuget.org/v3-flatcontainer/microsoft.windows.console.conpty/$Version/microsoft.windows.console.conpty.$Version.nupkg"
$DllEntry = 'runtimes/win-x64/native/conpty.dll'
$ExeEntry = 'build/native/runtimes/x64/OpenConsole.exe'

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
$Nupkg = Join-Path $OutDir "Microsoft.Windows.Console.ConPTY.$Version.nupkg"

function Assert-Sha256([string]$Path) {
    $actual = (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToUpperInvariant()
    if ($actual -ne $ExpectedSha256) {
        Write-Error "SHA-256 mismatch for $(Split-Path -Leaf $Path): expected $ExpectedSha256, got $actual"
    }
}

if (Test-Path $Nupkg) {
    try {
        Assert-Sha256 $Nupkg
    }
    catch {
        Remove-Item -Force $Nupkg
    }
}

if (-not (Test-Path $Nupkg)) {
    Write-Host "Downloading $Url"
    Invoke-WebRequest -Uri $Url -OutFile $Nupkg -UseBasicParsing
}

Assert-Sha256 $Nupkg

Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [System.IO.Compression.ZipFile]::OpenRead($Nupkg)
try {
    foreach ($pair in @(
            @{ Entry = $DllEntry; Name = 'conpty.dll' },
            @{ Entry = $ExeEntry; Name = 'OpenConsole.exe' }
        )) {
        $entry = $zip.GetEntry($pair.Entry)
        if ($null -eq $entry) {
            Write-Error "NuGet package is missing $($pair.Entry)"
        }
        $dest = Join-Path $OutDir $pair.Name
        if (Test-Path $dest) {
            Remove-Item -Force $dest
        }
        [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $dest)
        if (-not (Test-Path $dest) -or ((Get-Item $dest).Length -le 0)) {
            Write-Error "Failed to extract $($pair.Name)"
        }
    }
}
finally {
    $zip.Dispose()
}

Write-Host "ConPTY bundle ready in $OutDir (Microsoft.Windows.Console.ConPTY $Version)"

<#
.SYNOPSIS
Build and sign the WorkCentre 3119 WIA driver package without touching the system.

.DESCRIPTION
Copies the current release DLL and wc3119-wia.inf into a new package directory,
generates the catalog with the WDK Inf2Cat tool, signs the catalog with a code
signing certificate, and verifies the result. Nothing here installs a driver,
imports a certificate into machine stores, or changes device bindings.

The free signing path uses a self-signed test certificate. Every computer that
installs the package must first trust that certificate (see wc3119-setup.ps1
-TrustCertificate). Attestation signing would remove that step but costs money.

.PARAMETER OutputRoot
Directory that receives the new package directory. Defaults to artifacts/.

.PARAMETER CertificateThumbprint
Thumbprint of a code signing certificate in CurrentUser\My used to sign the
catalog. Required unless -NewTestCertificate is given.

.PARAMETER NewTestCertificate
Create a self-signed code signing certificate in CurrentUser\My, export its
public part next to the package, and use it for signing. This touches only the
current user's certificate store.

.PARAMETER Inf2Cat
Path to Inf2Cat.exe from the Windows Driver Kit. Auto-detected under
Windows Kits\10\bin\<version>\x86 when omitted.

.PARAMETER SkipCatalog
Only stage the package (INF + DLL + hashes). Use when the WDK is not installed
yet; the staged package cannot be installed until a signed catalog exists.

.PARAMETER DumpbinPath
Path to the Visual Studio dumpbin.exe used to verify the staged DLL imports.
Auto-detected from PATH or a standard Visual Studio 2022 installation.

.EXAMPLE
./driver/package.ps1 -NewTestCertificate

.EXAMPLE
./driver/package.ps1 -CertificateThumbprint 0123ABCD...
#>
[CmdletBinding()]
param(
    [string]$OutputRoot,
    [ValidatePattern('^[0-9A-Fa-f]{40}$')][string]$CertificateThumbprint,
    [switch]$NewTestCertificate,
    [string]$Inf2Cat,
    [switch]$SkipCatalog,
    [string]$DumpbinPath
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$dll = Join-Path $projectRoot 'target/release/workcentre_3119.dll'
$inf = Join-Path $PSScriptRoot 'wc3119-wia.inf'
if (-not $OutputRoot) { $OutputRoot = Join-Path $projectRoot 'artifacts' }

if (-not (Test-Path $dll)) { throw "Release DLL is missing; run cargo build --offline --release first: $dll" }
if (-not (Test-Path $inf)) { throw "INF is missing: $inf" }

if (-not $DumpbinPath) {
    $dumpbinCommand = Get-Command dumpbin.exe -ErrorAction SilentlyContinue
    if ($dumpbinCommand) {
        $DumpbinPath = $dumpbinCommand.Source
    } else {
        $vsRoot = Join-Path $env:ProgramFiles 'Microsoft Visual Studio\2022'
        $dumpbinCandidates = @(Get-ChildItem -Path (Join-Path $vsRoot '*\VC\Tools\MSVC\*\bin\Hostx64\x64\dumpbin.exe') `
            -ErrorAction SilentlyContinue | Sort-Object FullName -Descending)
        if ($dumpbinCandidates.Count -gt 0) { $DumpbinPath = $dumpbinCandidates[0].FullName }
    }
}
if (-not $DumpbinPath -or -not (Test-Path -LiteralPath $DumpbinPath -PathType Leaf)) {
    throw 'dumpbin.exe not found. Install Visual Studio C++ build tools, add dumpbin.exe to PATH, or pass -DumpbinPath.'
}

function Assert-NoVisualCppRuntimeImports {
    param([Parameter(Mandatory = $true)][string]$Path)

    $dumpbinOutput = @(& $DumpbinPath /DEPENDENTS $Path 2>&1)
    $dumpbinExitCode = $LASTEXITCODE
    if ($dumpbinExitCode -ne 0) {
        throw "dumpbin.exe /DEPENDENTS failed for package DLL '$Path' with exit code $dumpbinExitCode."
    }

    $dependencies = @()
    foreach ($line in $dumpbinOutput) {
        if ([string]$line -match '^\s*([A-Za-z0-9_.+-]+\.dll)\s*$') { $dependencies += $Matches[1] }
    }
    if ($dependencies.Count -eq 0) {
        throw "dumpbin.exe returned no DLL dependency names for package DLL '$Path'; its imports cannot be verified."
    }

    $runtimePattern = '^(?:VCRUNTIME|MSVCP|MSVCR|CONCRT|VCOMP|MFC|MFCS|ATL)\d+[A-Z0-9_]*\.DLL$|^(?:MSVCPRTD|MSVCRTD)\.DLL$'
    $runtimeImports = @($dependencies | Where-Object { $_ -match $runtimePattern })
    if ($runtimeImports.Count -gt 0) {
        throw ("Package DLL imports Visual C++ runtime DLL(s): {0}. Rebuild with the repository crt-static setting." -f `
            ($runtimeImports -join ', '))
    }
}

$infText = Get-Content $inf -Raw
if ($infText -notmatch '(?m)^DriverVer\s*=\s*(\d\d/\d\d/\d{4}),(\d+\.\d+\.\d+\.\d+)\s*$') { throw 'INF has no DriverVer line' }
$driverDate = $Matches[1]
$driverVersion = $Matches[2]
if ($infText -notmatch '(?m)^CatalogFile\s*=\s*(\S+)\s*$') { throw 'INF has no CatalogFile line' }
$catalogName = $Matches[1]
if ($infText -notmatch 'USB\\VID_0924&PID_4265&MI_00') { throw 'INF does not target the MI_00 scanner interface' }
$infDirectives = ($infText -split "`r?`n" | Where-Object { $_ -notmatch '^\s*;' }) -join "`n"
if ($infDirectives -match 'STI\.USBSection') { throw 'INF must not include STI.USBSection (usbscan.sys would replace WinUSB)' }

$stamp = (Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssZ')
$package = Join-Path $OutputRoot "wia-package-$driverVersion-$stamp"
New-Item -ItemType Directory -Path $package | Out-Null
Copy-Item $inf (Join-Path $package 'wc3119-wia.inf')
$packageDll = Join-Path $package 'workcentre_3119.dll'
Copy-Item $dll $packageDll
Assert-NoVisualCppRuntimeImports -Path $packageDll
# The package is self-contained: the setup script runs in package mode next
# to manifest.json, and the .cmd wrappers give the end user one-click
# install/uninstall with UAC elevation.
foreach ($name in @('wc3119-setup.ps1', 'install.cmd', 'uninstall.cmd', 'INSTALL.txt')) {
    Copy-Item (Join-Path $PSScriptRoot $name) (Join-Path $package $name)
}

$manifest = [ordered]@{
    driverVersion = $driverVersion
    driverDate    = $driverDate
    builtUtc      = $stamp
    dllSha256     = (Get-FileHash $packageDll).Hash
    infSha256     = (Get-FileHash (Join-Path $package 'wc3119-wia.inf')).Hash
    catalog       = $null
    certificate   = $null
}

if ($SkipCatalog) {
    $manifest | ConvertTo-Json | Set-Content (Join-Path $package 'manifest.json') -Encoding UTF8
    Write-Output "Staged unsigned package (not installable yet): $package"
    exit 0
}

if (-not $Inf2Cat) {
    $candidates = @(Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin\*\x86\Inf2Cat.exe' -ErrorAction SilentlyContinue | Sort-Object FullName -Descending)
    if ($candidates.Count -eq 0) { throw 'Inf2Cat.exe not found. Install the Windows Driver Kit (free) or pass -Inf2Cat, or use -SkipCatalog to stage only.' }
    $Inf2Cat = $candidates[0].FullName
}

$certificate = $null
if ($NewTestCertificate) {
    if ($CertificateThumbprint) { throw 'Use either -NewTestCertificate or -CertificateThumbprint' }
    $certificate = New-SelfSignedCertificate -Type CodeSigningCert -Subject 'CN=WorkCentre 3119 Modern Driver Project (Test)' `
        -CertStoreLocation 'Cert:\CurrentUser\My' -KeyLength 2048 -HashAlgorithm SHA256 -NotAfter (Get-Date).AddYears(2)
    Export-Certificate -Cert $certificate -FilePath (Join-Path $package 'wc3119-test.cer') | Out-Null
} else {
    if (-not $CertificateThumbprint) { throw 'Pass -CertificateThumbprint or -NewTestCertificate' }
    $certificate = Get-Item "Cert:\CurrentUser\My\$CertificateThumbprint"
    if (-not $certificate.HasPrivateKey) { throw 'Certificate has no private key' }
    Export-Certificate -Cert $certificate -FilePath (Join-Path $package 'wc3119-test.cer') | Out-Null
}

& $Inf2Cat "/driver:$package" '/os:10_X64' '/uselocaltime'
if ($LASTEXITCODE -ne 0) { throw "Inf2Cat failed with exit code $LASTEXITCODE" }
$catalog = Join-Path $package $catalogName
if (-not (Test-Path $catalog)) { throw "Inf2Cat did not produce $catalogName" }

$signtool = @(Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin\*\x64\signtool.exe' -ErrorAction SilentlyContinue | Sort-Object FullName -Descending)
if ($signtool.Count -eq 0) { throw 'signtool.exe not found in the Windows SDK' }
& $signtool[0].FullName sign /fd SHA256 /sha1 $certificate.Thumbprint /tr http://timestamp.digicert.com /td SHA256 $catalog
if ($LASTEXITCODE -ne 0) {
    # Timestamping needs network access; a package without a timestamp still
    # installs while the certificate is valid.
    & $signtool[0].FullName sign /fd SHA256 /sha1 $certificate.Thumbprint $catalog
    if ($LASTEXITCODE -ne 0) { throw "signtool failed with exit code $LASTEXITCODE" }
}
& $signtool[0].FullName verify /pa /v $catalog | Out-Null
$signature = Get-AuthenticodeSignature $catalog
if ($signature.SignerCertificate.Thumbprint -ne $certificate.Thumbprint) { throw 'Catalog signer does not match the requested certificate' }

$manifest.catalog = [ordered]@{ name = $catalogName; sha256 = (Get-FileHash $catalog).Hash }
$manifest.certificate = [ordered]@{ subject = $certificate.Subject; thumbprint = $certificate.Thumbprint; notAfter = $certificate.NotAfter.ToString('o') }
$manifest | ConvertTo-Json | Set-Content (Join-Path $package 'manifest.json') -Encoding UTF8
Assert-NoVisualCppRuntimeImports -Path $packageDll
if ((Get-FileHash $packageDll).Hash -ne $manifest.dllSha256) { throw 'DLL changed during packaging' }
Write-Output "Signed package: $package"
Write-Output "Signer: $($certificate.Subject) $($certificate.Thumbprint)"
Write-Output 'Next: copy the directory to the target PC and run install.cmd (or wc3119-setup.ps1 -Action Install -TrustCertificate -Apply) after authorization.'

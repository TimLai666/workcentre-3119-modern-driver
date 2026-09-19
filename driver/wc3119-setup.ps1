<#
.SYNOPSIS
Install, update, or uninstall the WorkCentre 3119 WIA driver package, with a
preflight-only default and a full backup before any change.

.DESCRIPTION
Actions:
  Status     Read-only report: device binding, installed package versions,
             certificate trust, CLSID registration, WIA service state.
  Install    Trust check, backup, pnputil /add-driver /install, verification.
  Update     Like Install, but requires a newer DriverVer than the installed
             package and removes the superseded package afterwards.
  Uninstall  pnputil /delete-driver /uninstall for this project's package,
             CLSID cleanup, verification that MI_00 is unbound again.

Without -Apply every action only performs its preflight and prints what would
happen. -Apply performs the change and needs an elevated PowerShell.

Certificate trust is a separate, explicit step (-TrustCertificate /
-UntrustCertificate) because it changes machine-wide security state.

Never binds the composite parent or the MI_01 printer interface. Every run
writes a log directory under artifacts/, which Git ignores.

.PARAMETER Package
Directory produced by package.ps1 (contains wc3119-wia.inf, catalog, DLL,
manifest.json, wc3119-test.cer). Required for Install/Update and for
-TrustCertificate.

.PARAMETER Action
Status (default), Install, Update, or Uninstall.

.PARAMETER Apply
Perform the change. Without it, only preflight runs.

.PARAMETER TrustCertificate
Import the package's wc3119-test.cer into LocalMachine\Root and
LocalMachine\TrustedPublisher so PnP accepts the signed catalog. Requires
-Apply and elevation.

.PARAMETER UntrustCertificate
Remove the project's test certificate from both machine stores. Requires
-Apply and elevation; do this after Uninstall when no package remains.

.EXAMPLE
./driver/wc3119-setup.ps1 -Action Status

.EXAMPLE
./driver/wc3119-setup.ps1 -Package artifacts/wia-package-0.2.0.0-... -TrustCertificate -Apply
./driver/wc3119-setup.ps1 -Package artifacts/wia-package-0.2.0.0-... -Action Install -Apply

.EXAMPLE
./driver/wc3119-setup.ps1 -Action Uninstall -Apply
#>
[CmdletBinding()]
param(
    [string]$Package,
    [ValidateSet('Status', 'Install', 'Update', 'Uninstall')][string]$Action = 'Status',
    [switch]$Apply,
    [switch]$TrustCertificate,
    [switch]$UntrustCertificate
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$scannerPattern = '^USB\\VID_0924&PID_4265&MI_00\\[^\\]+$'
$protectedPattern = '^USB\\VID_0924&PID_4265(&MI_01)?\\[^\\]+$'
$driverClsid = '{F71A8435-AA10-40A6-8334-49EEC8FE9C63}'
$imageClass = '{6bdd1fc6-810f-11d0-bec7-08002be2092f}'
$usbDeviceClass = '{88bae032-5a81-49f0-bc3d-a4ff138216d6}'
$provider = 'WorkCentre 3119 Modern Driver Project'
$testSubject = 'CN=WorkCentre 3119 Modern Driver Project (Test)'
$logDir = Join-Path $projectRoot ("artifacts/wia-setup-{0}-{1}" -f $Action.ToLower(), (Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssZ'))

function Write-Log([string]$Text) {
    if (-not (Test-Path $logDir)) { New-Item -ItemType Directory -Path $logDir | Out-Null }
    Add-Content -Path (Join-Path $logDir 'log.txt') -Value ("{0} {1}" -f (Get-Date).ToString('o'), $Text) -Encoding UTF8
    Write-Output $Text
}
function Test-Admin {
    ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}
function Get-Properties([string]$Id) {
    $result = @{}
    foreach ($property in @(Get-PnpDeviceProperty -InstanceId $Id)) { $result[$property.KeyName] = $property.Data }
    return $result
}
function Get-Scanner {
    $present = @(Get-PnpDevice -PresentOnly | Where-Object InstanceId -match $scannerPattern)
    if ($present.Count -ne 1) { throw "Expected exactly one present MI_00 scanner interface, found $($present.Count)" }
    return $present[0]
}
function Get-ProjectPackages {
    # pnputil prints blocks; parse Published Name / Original Name / Provider / Version.
    $blocks = @()
    $current = @{}
    foreach ($line in @(& pnputil.exe /enum-drivers)) {
        if ($line -match '^\s*Published Name:\s*(\S+)') { if ($current.Count) { $blocks += [pscustomobject]$current }; $current = @{ Published = $Matches[1] } }
        elseif ($line -match '^\s*Original Name:\s*(\S+)') { $current.Original = $Matches[1] }
        elseif ($line -match '^\s*Provider Name:\s*(.+?)\s*$') { $current.Provider = $Matches[1] }
        elseif ($line -match '^\s*Driver Version:\s*(\S+)\s+(\S+)') { $current.Date = $Matches[1]; $current.Version = [version]$Matches[2] }
    }
    if ($current.Count) { $blocks += [pscustomobject]$current }
    return @($blocks | Where-Object { $_.PSObject.Properties['Original'] -and $_.Original -eq 'wc3119-wia.inf' -and $_.Provider -eq $provider })
}
function Get-TrustedTestCertificates {
    @(Get-ChildItem Cert:\LocalMachine\Root, Cert:\LocalMachine\TrustedPublisher | Where-Object Subject -eq $testSubject)
}
function Get-WiaDeviceCount {
    $manager = $null
    try {
        $manager = New-Object -ComObject WIA.DeviceManager
        return [int]$manager.DeviceInfos.Count
    } catch { return -1 } finally {
        if ($manager) { [Runtime.InteropServices.Marshal]::ReleaseComObject($manager) | Out-Null }
    }
}
function Read-Package {
    if (-not $Package) { throw "-Package is required for $Action" }
    $dir = (Resolve-Path $Package).Path
    $manifestPath = Join-Path $dir 'manifest.json'
    if (-not (Test-Path $manifestPath)) { throw 'manifest.json is missing; build the package with package.ps1' }
    $manifest = Get-Content $manifestPath -Raw | ConvertFrom-Json
    if ($null -eq $manifest.catalog) { throw 'Package has no signed catalog; PnP will refuse it' }
    foreach ($name in @('wc3119-wia.inf', 'workcentre_3119.dll', $manifest.catalog.name, 'wc3119-test.cer')) {
        if (-not (Test-Path (Join-Path $dir $name))) { throw "Package file missing: $name" }
    }
    if ((Get-FileHash (Join-Path $dir 'workcentre_3119.dll')).Hash -ne $manifest.dllSha256) { throw 'DLL hash differs from manifest' }
    if ((Get-FileHash (Join-Path $dir 'wc3119-wia.inf')).Hash -ne $manifest.infSha256) { throw 'INF hash differs from manifest' }
    if ((Get-FileHash (Join-Path $dir $manifest.catalog.name)).Hash -ne $manifest.catalog.sha256) { throw 'Catalog hash differs from manifest' }
    $signature = Get-AuthenticodeSignature (Join-Path $dir $manifest.catalog.name)
    if ($null -eq $signature.SignerCertificate) { throw 'Catalog is not signed' }
    return [pscustomobject]@{ Dir = $dir; Manifest = $manifest; Signature = $signature; Version = [version]$manifest.driverVersion }
}
function Save-Backup {
    $scanner = Get-Scanner
    $devices = @(Get-PnpDevice -PresentOnly | Where-Object InstanceId -match '^USB\\VID_0924&PID_4265' | ForEach-Object {
            [pscustomobject]@{ InstanceId = $_.InstanceId; Properties = @(Get-PnpDeviceProperty -InstanceId $_.InstanceId) }
        })
    $devices | Export-Clixml (Join-Path $logDir 'devices-before.clixml')
    & pnputil.exe /enum-drivers | Set-Content (Join-Path $logDir 'drivers-before.txt') -Encoding UTF8
    & reg.exe export "HKLM\SYSTEM\CurrentControlSet\Enum\$($scanner.InstanceId)" (Join-Path $logDir 'scanner-before.reg') /y | Out-Null
    $clsidExists = Test-Path "Registry::HKEY_CLASSES_ROOT\CLSID\$driverClsid"
    if ($clsidExists) { & reg.exe export "HKCR\CLSID\$driverClsid" (Join-Path $logDir 'clsid-before.reg') /y | Out-Null }
    [pscustomobject]@{
        scanner       = $scanner.InstanceId
        clsidExisted  = $clsidExists
        stisvc        = (Get-Service stisvc).Status.ToString()
        wiaDevices    = Get-WiaDeviceCount
        trustedCerts  = @(Get-TrustedTestCertificates | ForEach-Object Thumbprint)
        packages      = @(Get-ProjectPackages)
    } | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $logDir 'state-before.json') -Encoding UTF8
    Write-Log "Backup written to $logDir (may contain serial numbers; not for Git)"
}
function Assert-ProtectedUnchanged {
    $before = @(Import-Clixml (Join-Path $logDir 'devices-before.clixml'))
    foreach ($item in @($before | Where-Object InstanceId -match $protectedPattern)) {
        $live = Get-Properties $item.InstanceId
        foreach ($name in @('Service', 'DriverInfPath', 'ProblemCode', 'ClassGuid', 'Parent')) {
            $saved = @($item.Properties | Where-Object KeyName -eq "DEVPKEY_Device_$name")
            $expected = if ($saved.Count) { $saved[0].Data } else { $null }
            if ($live["DEVPKEY_Device_$name"] -ne $expected) { throw "Protected device changed after the operation: $($item.InstanceId) $name" }
        }
    }
    Write-Log 'Composite parent and MI_01 unchanged'
}
function Show-Status {
    $scanner = Get-Scanner
    $properties = Get-Properties $scanner.InstanceId
    Write-Log ("MI_00: service={0} inf={1} class={2} problem={3}" -f $properties['DEVPKEY_Device_Service'], $properties['DEVPKEY_Device_DriverInfPath'], $properties['DEVPKEY_Device_ClassGuid'], $properties['DEVPKEY_Device_ProblemCode'])
    $packages = @(Get-ProjectPackages)
    if ($packages.Count -eq 0) { Write-Log 'Project package: not installed' }
    foreach ($item in $packages) { Write-Log ("Project package: {0} version {1} ({2})" -f $item.Published, $item.Version, $item.Date) }
    Write-Log ("CLSID registered: {0}" -f (Test-Path "Registry::HKEY_CLASSES_ROOT\CLSID\$driverClsid"))
    Write-Log ("Trusted test certificates: {0}" -f (@(Get-TrustedTestCertificates).Count))
    Write-Log ("stisvc: {0}; WIA devices visible: {1}" -f (Get-Service stisvc).Status, (Get-WiaDeviceCount))
    Write-Log ("Elevated: {0}" -f (Test-Admin))
}
function Invoke-Pnputil([string[]]$Arguments) {
    Write-Log ("pnputil {0}" -f ($Arguments -join ' '))
    $output = & pnputil.exe @Arguments 2>&1
    $code = $LASTEXITCODE
    $output | Add-Content (Join-Path $logDir 'pnputil.txt') -Encoding UTF8
    Write-Log "pnputil exit code $code"
    # 0 = success, 259 (ERROR_NO_MORE_ITEMS) = nothing matched, 3010 = reboot required
    if ($code -notin @(0, 3010)) { throw "pnputil failed with $code; see $logDir\pnputil.txt" }
    return $code
}
function Verify-Installed($Expected) {
    $scanner = Get-Scanner
    $properties = Get-Properties $scanner.InstanceId
    if ($properties['DEVPKEY_Device_Service'] -ne 'WINUSB') { throw "MI_00 service is $($properties['DEVPKEY_Device_Service']); expected WINUSB" }
    if ("$($properties['DEVPKEY_Device_ClassGuid'])".ToLower() -ne $imageClass) { throw "MI_00 class is $($properties['DEVPKEY_Device_ClassGuid']); expected Image" }
    if ($properties['DEVPKEY_Device_ProblemCode'] -ne 0) { throw "MI_00 problem code $($properties['DEVPKEY_Device_ProblemCode'])" }
    $packages = @(Get-ProjectPackages | Where-Object Version -eq $Expected.Version)
    if ($packages.Count -ne 1) { throw "Installed package version $($Expected.Version) not found in the driver store" }
    if (-not (Test-Path "Registry::HKEY_CLASSES_ROOT\CLSID\$driverClsid\InProcServer32")) { throw 'CLSID InProcServer32 was not registered' }
    $server = (Get-ItemProperty "Registry::HKEY_CLASSES_ROOT\CLSID\$driverClsid\InProcServer32").'(default)'
    Write-Log "COM server path: $server"
    $count = Get-WiaDeviceCount
    Write-Log "WIA devices visible: $count"
    if ($count -lt 1) { throw 'WIA does not list the scanner yet; check stisvc and the Windows Image Acquisition event log before retrying' }
    Write-Log 'Installed and visible to WIA. Next: scan once at 75 dpi with Windows Scan, cancel once, and scan again.'
}

try {
    if (($TrustCertificate -or $UntrustCertificate) -and -not $Apply) { throw 'Certificate trust changes require -Apply' }
    if ($TrustCertificate -and $UntrustCertificate) { throw 'Choose one of -TrustCertificate / -UntrustCertificate' }
    if ($TrustCertificate) {
        if (-not (Test-Admin)) { throw 'Elevation is required to trust a machine-wide certificate' }
        $info = Read-Package
        $cer = Join-Path $info.Dir 'wc3119-test.cer'
        $certificate = New-Object Security.Cryptography.X509Certificates.X509Certificate2 $cer
        if ($certificate.Thumbprint -ne $info.Signature.SignerCertificate.Thumbprint) { throw 'Package certificate does not match the catalog signer' }
        if ($certificate.Subject -ne $testSubject) { throw "Refusing to trust an unexpected subject: $($certificate.Subject)" }
        foreach ($store in @('Root', 'TrustedPublisher')) {
            Import-Certificate -FilePath $cer -CertStoreLocation "Cert:\LocalMachine\$store" | Out-Null
            Write-Log "Imported $($certificate.Thumbprint) into LocalMachine\$store"
        }
        exit 0
    }
    if ($UntrustCertificate) {
        if (-not (Test-Admin)) { throw 'Elevation is required to change machine certificate stores' }
        if (@(Get-ProjectPackages).Count -ne 0) { throw 'Uninstall the driver package before removing certificate trust' }
        foreach ($certificate in Get-TrustedTestCertificates) {
            Remove-Item $certificate.PSPath
            Write-Log "Removed $($certificate.Thumbprint) from $($certificate.PSParentPath)"
        }
        exit 0
    }

    switch ($Action) {
        'Status' { Show-Status; exit 0 }
        'Install' {
            $info = Read-Package
            Show-Status
            if (@(Get-ProjectPackages).Count -ne 0) { throw 'A project package is already installed; use -Action Update or Uninstall' }
            $trusted = @(Get-TrustedTestCertificates | Where-Object Thumbprint -eq $info.Signature.SignerCertificate.Thumbprint)
            if ($trusted.Count -lt 2) { throw 'The catalog signer is not trusted in both LocalMachine\Root and TrustedPublisher; run -TrustCertificate -Apply first' }
            $scanner = Get-Scanner
            $properties = Get-Properties $scanner.InstanceId
            if ($properties['DEVPKEY_Device_Service'] -ne 'WINUSB' -and $properties['DEVPKEY_Device_ProblemCode'] -ne 28) { throw "MI_00 is bound to $($properties['DEVPKEY_Device_Service']); needs separate review" }
            if (-not $Apply) { Write-Log "Preflight passed. Would run pnputil /add-driver $($info.Dir)\wc3119-wia.inf /install"; exit 0 }
            if (-not (Test-Admin)) { throw 'Elevation is required' }
            Save-Backup
            $code = Invoke-Pnputil @('/add-driver', (Join-Path $info.Dir 'wc3119-wia.inf'), '/install')
            Assert-ProtectedUnchanged
            if ($code -eq 3010) { Write-Log 'Windows requests a reboot; not rebooting automatically. Re-run -Action Status after the reboot.'; exit 3010 }
            Verify-Installed $info
            exit 0
        }
        'Update' {
            $info = Read-Package
            Show-Status
            $installed = @(Get-ProjectPackages)
            if ($installed.Count -eq 0) { throw 'No project package is installed; use -Action Install' }
            $newest = ($installed | Sort-Object Version -Descending)[0]
            if ($info.Version -le $newest.Version) { throw "Package version $($info.Version) is not newer than installed $($newest.Version); bump DriverVer in the INF" }
            $trusted = @(Get-TrustedTestCertificates | Where-Object Thumbprint -eq $info.Signature.SignerCertificate.Thumbprint)
            if ($trusted.Count -lt 2) { throw 'The catalog signer is not trusted; run -TrustCertificate -Apply first' }
            if (-not $Apply) { Write-Log "Preflight passed. Would install $($info.Version) over $($newest.Version) and delete $($newest.Published)"; exit 0 }
            if (-not (Test-Admin)) { throw 'Elevation is required' }
            Save-Backup
            $code = Invoke-Pnputil @('/add-driver', (Join-Path $info.Dir 'wc3119-wia.inf'), '/install')
            Assert-ProtectedUnchanged
            if ($code -eq 3010) { Write-Log 'Windows requests a reboot before the new DLL is in use; not rebooting automatically. Re-run Update after the reboot to remove the superseded package.'; exit 3010 }
            Verify-Installed $info
            foreach ($old in @(Get-ProjectPackages | Where-Object Version -lt $info.Version)) {
                Invoke-Pnputil @('/delete-driver', $old.Published) | Out-Null
                Write-Log "Removed superseded package $($old.Published) ($($old.Version))"
            }
            exit 0
        }
        'Uninstall' {
            Show-Status
            $installed = @(Get-ProjectPackages)
            if ($installed.Count -eq 0 -and -not (Test-Path "Registry::HKEY_CLASSES_ROOT\CLSID\$driverClsid")) { Write-Log 'Nothing to uninstall'; exit 0 }
            if (-not $Apply) { Write-Log ("Preflight passed. Would delete {0} and the CLSID key; MI_00 returns to no driver (problem 28) until re-paired" -f (($installed | ForEach-Object Published) -join ', ')); exit 0 }
            if (-not (Test-Admin)) { throw 'Elevation is required' }
            Save-Backup
            foreach ($item in $installed) { Invoke-Pnputil @('/delete-driver', $item.Published, '/uninstall', '/force') | Out-Null }
            # INF AddReg entries under HKCR are not removed by device uninstall.
            if (Test-Path "Registry::HKEY_CLASSES_ROOT\CLSID\$driverClsid") {
                Remove-Item "Registry::HKEY_CLASSES_ROOT\CLSID\$driverClsid" -Recurse
                Write-Log "Removed HKCR\CLSID\$driverClsid"
            }
            Assert-ProtectedUnchanged
            if (@(Get-ProjectPackages).Count -ne 0) { throw 'Driver store still lists a project package' }
            $properties = Get-Properties (Get-Scanner).InstanceId
            Write-Log ("MI_00 after uninstall: service={0} class={1} problem={2}" -f $properties['DEVPKEY_Device_Service'], $properties['DEVPKEY_Device_ClassGuid'], $properties['DEVPKEY_Device_ProblemCode'])
            if ("$($properties['DEVPKEY_Device_ClassGuid'])".ToLower() -eq $imageClass) { Write-Log 'Devnode still carries the Image class; re-pair with examples/winusb_setup.rs if WinUSB-only access is wanted' }
            Write-Log 'Uninstalled. Certificate trust is kept; remove it with -UntrustCertificate -Apply if no longer needed.'
            exit 0
        }
    }
} catch {
    Write-Log "FAILED: $($_.Exception.Message)"
    exit 1
}

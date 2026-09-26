<#
.SYNOPSIS
Install, update, or uninstall the WorkCentre 3119 WIA driver package, with a
preflight-only default and a full backup before any change.

.DESCRIPTION
Actions:
  Status     Read-only report: device binding, installed package versions,
             certificate trust, CLSID registration, WIA service state.
  Install    One-shot setup. Trusts the package certificate when
             -TrustCertificate is given, then installs the package, or updates
             an older installed package, or verifies an identical one. Stages
             the driver when the scanner is not plugged in yet; Windows binds
             it automatically on arrival.
  Update     Like Install, but requires a newer DriverVer than the installed
             package and removes the superseded package afterwards.
  Uninstall  pnputil /delete-driver /uninstall for this project's package,
             CLSID cleanup, verification that MI_00 is unbound again. Add
             -UntrustCertificate to also remove the certificate trust.

Without -Apply every action only performs its preflight and prints what would
happen. -Apply performs the change and needs an elevated PowerShell.

Certificate trust is an explicit switch (-TrustCertificate /
-UntrustCertificate) because it changes machine-wide security state. It can
be combined with Install/Update/Uninstall so one elevated run does everything
(this is what install.cmd / uninstall.cmd in the package do).

Package mode: when this script sits inside a package directory (next to
manifest.json) it uses that directory as -Package and writes its logs under
%ProgramData%\WorkCentre3119Driver\setup-logs. Inside the repository it keeps
writing under artifacts/, which Git ignores.

Never binds the composite parent or the MI_01 printer interface.

.PARAMETER Package
Directory produced by package.ps1 (contains wc3119-wia.inf, catalog, DLL,
manifest.json, wc3119-test.cer). Defaults to the script directory in package
mode. Required for Install/Update and for -TrustCertificate otherwise.

.PARAMETER Action
Status (default), Install, Update, or Uninstall.

.PARAMETER Apply
Perform the change. Without it, only preflight runs.

.PARAMETER TrustCertificate
Import the package's wc3119-test.cer into LocalMachine\Root and
LocalMachine\TrustedPublisher so PnP accepts the signed catalog. Requires
-Apply and elevation. Skipped silently when already trusted.

.PARAMETER UntrustCertificate
Remove the project's test certificate from both machine stores. Requires
-Apply and elevation; combine with -Action Uninstall or run it after.

.EXAMPLE
./driver/wc3119-setup.ps1 -Action Status

.EXAMPLE
./driver/wc3119-setup.ps1 -Package artifacts/wia-package-0.2.0.0-... -Action Install -TrustCertificate -Apply

.EXAMPLE
./driver/wc3119-setup.ps1 -Action Uninstall -UntrustCertificate -Apply
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

$packageMode = Test-Path (Join-Path $PSScriptRoot 'manifest.json')
if ($packageMode -and -not $Package) { $Package = $PSScriptRoot }
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$logRoot = if ($packageMode) { Join-Path $env:ProgramData 'WorkCentre3119Driver\setup-logs' } else { Join-Path $projectRoot 'artifacts' }
$scannerPattern = '^USB\\VID_0924&PID_4265&MI_00\\[^\\]+$'
$protectedPattern = '^USB\\VID_0924&PID_4265(&MI_01)?\\[^\\]+$'
$driverClsid = '{F71A8435-AA10-40A6-8334-49EEC8FE9C63}'
$imageClass = '{6bdd1fc6-810f-11d0-bec7-08002be2092f}'
$usbDeviceClass = '{88bae032-5a81-49f0-bc3d-a4ff138216d6}'
$provider = 'WorkCentre 3119 Modern Driver Project'
$testSubject = 'CN=WorkCentre 3119 Modern Driver Project (Test)'
$logDir = Join-Path $logRoot ("wia-setup-{0}-{1}" -f $Action.ToLower(), (Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssZ'))

function Write-Log([string]$Text) {
    if (-not (Test-Path $logDir)) { New-Item -ItemType Directory -Path $logDir -Force | Out-Null }
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
function Get-ScannerOrNull {
    $present = @(Get-PnpDevice -PresentOnly | Where-Object InstanceId -match $scannerPattern)
    if ($present.Count -gt 1) { throw "Expected at most one present MI_00 scanner interface, found $($present.Count)" }
    if ($present.Count -eq 0) { return $null }
    return $present[0]
}
function Get-Scanner {
    $scanner = Get-ScannerOrNull
    if ($null -eq $scanner) { throw 'The scanner is not connected (no present MI_00 interface)' }
    return $scanner
}
function Get-ProjectPackages {
    # Language-neutral: read the staged oem*.inf files instead of parsing the
    # localized pnputil /enum-drivers text.
    $result = @()
    foreach ($file in @(Get-ChildItem (Join-Path $env:windir 'INF\oem*.inf') -ErrorAction SilentlyContinue)) {
        $text = Get-Content $file.FullName -Raw -ErrorAction SilentlyContinue
        if (-not $text) { continue }
        if ($text -notmatch '(?m)^CatalogFile\s*=\s*wc3119-wia\.cat\s*$') { continue }
        if ($text -notmatch [regex]::Escape($provider)) { continue }
        if ($text -notmatch '(?m)^DriverVer\s*=\s*(\d\d/\d\d/\d{4}),(\d+\.\d+\.\d+\.\d+)\s*$') { continue }
        $result += [pscustomobject]@{ Published = $file.Name; Original = 'wc3119-wia.inf'; Provider = $provider; Date = $Matches[1]; Version = [version]$Matches[2] }
    }
    return @($result)
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
    $scanner = Get-ScannerOrNull
    $devices = @(Get-PnpDevice -PresentOnly | Where-Object InstanceId -match '^USB\\VID_0924&PID_4265' | ForEach-Object {
            [pscustomobject]@{ InstanceId = $_.InstanceId; Properties = @(Get-PnpDeviceProperty -InstanceId $_.InstanceId) }
        })
    $devices | Export-Clixml (Join-Path $logDir 'devices-before.clixml')
    & pnputil.exe /enum-drivers | Set-Content (Join-Path $logDir 'drivers-before.txt') -Encoding UTF8
    if ($scanner) { & reg.exe export "HKLM\SYSTEM\CurrentControlSet\Enum\$($scanner.InstanceId)" (Join-Path $logDir 'scanner-before.reg') /y | Out-Null }
    $clsidExists = Test-Path "Registry::HKEY_CLASSES_ROOT\CLSID\$driverClsid"
    if ($clsidExists) { & reg.exe export "HKCR\CLSID\$driverClsid" (Join-Path $logDir 'clsid-before.reg') /y | Out-Null }
    [pscustomobject]@{
        scanner       = if ($scanner) { $scanner.InstanceId } else { $null }
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
    $scanner = Get-ScannerOrNull
    if ($scanner) {
        $properties = Get-Properties $scanner.InstanceId
        Write-Log ("MI_00: service={0} inf={1} class={2} problem={3}" -f $properties['DEVPKEY_Device_Service'], $properties['DEVPKEY_Device_DriverInfPath'], $properties['DEVPKEY_Device_ClassGuid'], $properties['DEVPKEY_Device_ProblemCode'])
    } else {
        Write-Log 'MI_00: scanner not connected'
    }
    $packages = @(Get-ProjectPackages)
    if ($packages.Count -eq 0) { Write-Log 'Project package: not installed' }
    foreach ($item in $packages) { Write-Log ("Project package: {0} version {1} ({2})" -f $item.Published, $item.Version, $item.Date) }
    Write-Log ("CLSID registered: {0}" -f (Test-Path "Registry::HKEY_CLASSES_ROOT\CLSID\$driverClsid"))
    Write-Log ("Trusted test certificates: {0}" -f (@(Get-TrustedTestCertificates).Count))
    Write-Log ("stisvc: {0}; WIA devices visible: {1}" -f (Get-Service stisvc).Status, (Get-WiaDeviceCount))
    Write-Log ("Elevated: {0}" -f (Test-Admin))
}
function Invoke-Pnputil([string[]]$Arguments) {
    Write-Log ("pnputil {0}" -f ($Arguments -join ' ')) | Out-Null
    $output = & pnputil.exe @Arguments 2>&1
    $code = $LASTEXITCODE
    $output | Add-Content (Join-Path $logDir 'pnputil.txt') -Encoding UTF8
    Write-Log "pnputil exit code $code" | Out-Null
    # 0 = success, 259 (ERROR_NO_MORE_ITEMS) = staged but no matching device
    # present, 3010 = reboot required
    if ($code -notin @(0, 259, 3010)) { throw "pnputil failed with $code; see $logDir\pnputil.txt" }
    return $code
}
function Stop-WiaService([string]$Message = 'Stopping the Windows Image Acquisition service (stisvc) before the driver change') {
    # The loaded driver holds the single WinUSB handle; while it is open PnP
    # cannot restart the devnode and pnputil answers 3010 (reboot required).
    Write-Log $Message
    $status = [string](Get-Service stisvc -ErrorAction Stop).Status
    if ($status -eq 'Running') {
        Stop-Service stisvc -Force -ErrorAction Stop
    } elseif ($status -ne 'Stopped') {
        throw "Cannot safely stop stisvc while its state is $status"
    }
}
function Restore-WiaServiceState([bool]$WasRunning) {
    $status = [string](Get-Service stisvc -ErrorAction Stop).Status
    if ($WasRunning -and $status -ne 'Running') {
        Start-Service stisvc -ErrorAction Stop
    } elseif (-not $WasRunning -and $status -ne 'Stopped') {
        Stop-Service stisvc -Force -ErrorAction Stop
    }
}
function Invoke-WiaServiceOperation([scriptblock]$Operation, [bool]$StopFirst = $true, [string]$StopMessage = 'Stopping the Windows Image Acquisition service (stisvc) before the driver change') {
    $status = [string](Get-Service stisvc -ErrorAction Stop).Status
    if ($status -notin @('Running', 'Stopped')) {
        throw "Cannot safely change stisvc while its state is $status"
    }
    $wasRunning = $status -eq 'Running'
    try {
        if ($StopFirst) { Stop-WiaService $StopMessage }
        & $Operation
    } catch {
        $operationError = $_
        try {
            Restore-WiaServiceState $wasRunning
        } catch {
            $recoveryMessage = $_.Exception.Message
            try { Write-Log "Secondary failure restoring stisvc: $recoveryMessage" | Out-Null } catch {}
        }
        throw $operationError
    }
}
function Complete-PendingDeviceChange {
    # pnputil answered 3010: the devnode could not be restarted in place.
    # Removing the interface devnode and rescanning re-enumerates it with the
    # new binding without a reboot (verified 2026-09-19). Returns $true when the
    # device is back without a pending operation.
    $scanner = Get-Scanner
    $parent = (Get-Properties $scanner.InstanceId)['DEVPKEY_Device_Parent']
    # Write-Log emits to the pipeline; discard it so the boolean result stays clean.
    Write-Log "Re-enumerating $($scanner.InstanceId) to complete the pending change" | Out-Null
    Invoke-Pnputil @('/remove-device', $scanner.InstanceId) | Out-Null
    Start-Sleep -Seconds 2
    Invoke-Pnputil @('/scan-devices') | Out-Null
    Start-Sleep -Seconds 4
    if (@(Get-PnpDevice -PresentOnly | Where-Object InstanceId -match $scannerPattern).Count -eq 0 -and $parent) {
        # A removed interface devnode of a composite device only reappears
        # when the usbccgp parent re-enumerates its interfaces (2026-09-19).
        # MI_01 restarts with it; Assert-ProtectedUnchanged verifies it after.
        Write-Log 'MI_00 did not reappear after the scan; restarting the composite parent' | Out-Null
        Invoke-Pnputil @('/restart-device', "$parent") | Out-Null
    }
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 500
        $present = @(Get-PnpDevice -PresentOnly | Where-Object InstanceId -match $scannerPattern)
        if ($present.Count -eq 1 -and $present[0].Status -eq 'OK') {
            $properties = Get-Properties $present[0].InstanceId
            if ("$($properties['DEVPKEY_Device_Service'])" -eq 'WINUSB') { return $true }
        }
    }
    return $false
}
function Restart-WiaService {
    $operation = {
        # COM keeps the previous in-process server mapped inside the WIA service,
        # and a driver that failed to load is dropped from the service's device
        # list until it re-enumerates. Restarting stisvc makes it load the DLL
        # that the CLSID now points at. Applications must not be scanning.
        Write-Log 'Restarting the Windows Image Acquisition service (stisvc)'
        Restart-Service stisvc -Force -ErrorAction Stop
        $deadline = (Get-Date).AddSeconds(20)
        while ((Get-Date) -lt $deadline -and (Get-WiaDeviceCount) -lt 1) { Start-Sleep -Milliseconds 500 }
    }
    Invoke-WiaServiceOperation -Operation $operation -StopFirst $false
}
function Verify-Installed($Expected) {
    $packages = @(Get-ProjectPackages | Where-Object Version -eq $Expected.Version)
    if ($packages.Count -ne 1) { throw "Installed package version $($Expected.Version) not found in the driver store" }
    $scanner = Get-ScannerOrNull
    if ($null -eq $scanner) {
        Write-Log "Package $($Expected.Version) is staged. Connect the scanner; Windows binds it automatically and the WIA service loads the driver."
        return
    }
    $properties = Get-Properties $scanner.InstanceId
    if ($properties['DEVPKEY_Device_Service'] -ne 'WINUSB') { throw "MI_00 service is $($properties['DEVPKEY_Device_Service']); expected WINUSB" }
    if ("$($properties['DEVPKEY_Device_ClassGuid'])".ToLower() -ne $imageClass) { throw "MI_00 class is $($properties['DEVPKEY_Device_ClassGuid']); expected Image" }
    if ($properties['DEVPKEY_Device_ProblemCode'] -ne 0) { throw "MI_00 problem code $($properties['DEVPKEY_Device_ProblemCode'])" }
    if (-not (Test-Path "Registry::HKEY_CLASSES_ROOT\CLSID\$driverClsid\InProcServer32")) { throw 'CLSID InProcServer32 was not registered' }
    $server = (Get-ItemProperty "Registry::HKEY_CLASSES_ROOT\CLSID\$driverClsid\InProcServer32").'(default)'
    Write-Log "COM server path: $server"
    $count = Get-WiaDeviceCount
    Write-Log "WIA devices visible: $count"
    if ($count -lt 1) { throw 'WIA does not list the scanner yet; check stisvc and the Windows Image Acquisition event log before retrying' }
    Write-Log 'Installed and visible to WIA.'
}
function Show-ScanAppHints {
    # The driver is complete; these are the stock Windows clients that use it.
    $hints = @()
    $scanApp = @(Get-AppxPackage -Name Microsoft.WindowsScan -ErrorAction SilentlyContinue)
    if ($scanApp.Count -gt 0) { $hints += 'Windows Scan app (Microsoft Store): installed' } else { $hints += 'Windows Scan app: not installed; get it from the Microsoft Store (Microsoft Scan, free) if you want the modern app' }
    $faxScan = Test-Path (Join-Path $env:windir 'System32\WFS.exe')
    if ($faxScan) { $hints += 'Windows Fax and Scan: installed' } else { $hints += 'Windows Fax and Scan: not installed; add the optional feature Print.Fax.Scan if you want the classic tool' }
    foreach ($line in $hints) { Write-Log $line }
}
function Grant-Trust($Info) {
    $cer = Join-Path $Info.Dir 'wc3119-test.cer'
    $certificate = New-Object Security.Cryptography.X509Certificates.X509Certificate2 $cer
    if ($certificate.Thumbprint -ne $Info.Signature.SignerCertificate.Thumbprint) { throw 'Package certificate does not match the catalog signer' }
    if ($certificate.Subject -ne $testSubject) { throw "Refusing to trust an unexpected subject: $($certificate.Subject)" }
    foreach ($store in @('Root', 'TrustedPublisher')) {
        $present = @(Get-ChildItem "Cert:\LocalMachine\$store" | Where-Object Thumbprint -eq $certificate.Thumbprint)
        if ($present.Count -gt 0) { Write-Log "Certificate $($certificate.Thumbprint) already trusted in LocalMachine\$store"; continue }
        Import-Certificate -FilePath $cer -CertStoreLocation "Cert:\LocalMachine\$store" | Out-Null
        Write-Log "Imported $($certificate.Thumbprint) into LocalMachine\$store"
    }
}
function Revoke-Trust {
    if (@(Get-ProjectPackages).Count -ne 0) { throw 'Uninstall the driver package before removing certificate trust' }
    $certificates = @(Get-TrustedTestCertificates)
    if ($certificates.Count -eq 0) { Write-Log 'No trusted test certificate to remove' }
    foreach ($certificate in $certificates) {
        Remove-Item $certificate.PSPath
        Write-Log "Removed $($certificate.Thumbprint) from $($certificate.PSParentPath)"
    }
}
function Assert-Trusted($Info) {
    $trusted = @(Get-TrustedTestCertificates | Where-Object Thumbprint -eq $Info.Signature.SignerCertificate.Thumbprint)
    if ($trusted.Count -lt 2) { throw 'The catalog signer is not trusted in both LocalMachine\Root and TrustedPublisher; add -TrustCertificate (install.cmd does this)' }
}
function Assert-BindingAcceptable {
    $scanner = Get-ScannerOrNull
    if ($null -eq $scanner) { Write-Log 'Scanner not connected: the package will be staged and bound automatically when it is plugged in'; return }
    $properties = Get-Properties $scanner.InstanceId
    $service = "$($properties['DEVPKEY_Device_Service'])"
    # Acceptable starting points: paired WinUSB, or no driver at all
    # (problem 28, or the transient no-service state right after an
    # uninstall). Anything else is bound to a foreign driver.
    if ($service -ne 'WINUSB' -and $service -ne '') { throw "MI_00 is bound to $service (another scanner driver); uninstall that driver first, then run again" }
}
function Install-Package($Info, $Superseded) {
    # $Superseded: installed project packages older than $Info (empty for a
    # fresh install). Shared by Install and Update. Log lines flow to the
    # pipeline (console); the exit code is passed back in $script:installExit
    # so it is not mixed into that output.
    $script:installExit = 1
    Save-Backup
    $scanner = Get-ScannerOrNull
    $operation = {
        $code = Invoke-Pnputil @('/add-driver', (Join-Path $Info.Dir 'wc3119-wia.inf'), '/install')
        Assert-ProtectedUnchanged
        if ($scanner) {
            if ($code -eq 3010 -and -not (Complete-PendingDeviceChange)) {
                Start-Service stisvc -ErrorAction Stop
                Write-Log 'Windows still requests a reboot before the new driver is in use; not rebooting automatically. Re-run after the reboot to verify and clean up.'
                $script:installExit = 3010
                return
            }
            Restart-WiaService
        }
        Verify-Installed $Info
        foreach ($old in @($Superseded)) {
            Invoke-Pnputil @('/delete-driver', $old.Published) | Out-Null
            Write-Log "Removed superseded package $($old.Published) ($($old.Version))"
        }
        Show-ScanAppHints
        $script:installExit = 0
    }
    if ($scanner) { Invoke-WiaServiceOperation -Operation $operation }
    else { & $operation }
}
function Invoke-DriverUninstall($Installed) {
    Save-Backup
    $operation = {
        $codes = @()
        foreach ($item in $Installed) { $codes += Invoke-Pnputil @('/delete-driver', $item.Published, '/uninstall', '/force') }
        # Re-enumerate so the devnode leaves the transient no-driver state
        # without a reboot when Windows allows it.
        Invoke-Pnputil @('/scan-devices') | Out-Null
        if ($codes -contains 3010) {
            $scanner = @(Get-PnpDevice -PresentOnly | Where-Object InstanceId -match $scannerPattern)
            if ($scanner.Count -eq 1) { Invoke-Pnputil @('/remove-device', $scanner[0].InstanceId) | Out-Null; Start-Sleep -Seconds 2; Invoke-Pnputil @('/scan-devices') | Out-Null }
        }
        Start-Service stisvc -ErrorAction Stop
        # INF AddReg entries under HKCR are not removed by device uninstall.
        if (Test-Path "Registry::HKEY_CLASSES_ROOT\CLSID\$driverClsid") {
            Remove-Item "Registry::HKEY_CLASSES_ROOT\CLSID\$driverClsid" -Recurse
            Write-Log "Removed HKCR\CLSID\$driverClsid"
        }
        Assert-ProtectedUnchanged
        if (@(Get-ProjectPackages).Count -ne 0) { throw 'Driver store still lists a project package' }
        $scanner = Get-ScannerOrNull
        if ($scanner) {
            $properties = Get-Properties $scanner.InstanceId
            Write-Log ("MI_00 after uninstall: service={0} class={1} problem={2}" -f $properties['DEVPKEY_Device_Service'], $properties['DEVPKEY_Device_ClassGuid'], $properties['DEVPKEY_Device_ProblemCode'])
            if ("$($properties['DEVPKEY_Device_ClassGuid'])".ToLower() -eq $imageClass) { Write-Log 'Devnode still carries the Image class; re-pair with examples/winusb_setup.rs if WinUSB-only access is wanted' }
        }
        if ($UntrustCertificate) { Revoke-Trust; Write-Log 'Uninstalled and certificate trust removed.' }
        else { Write-Log 'Uninstalled. Certificate trust is kept; remove it with -UntrustCertificate -Apply if no longer needed.' }
    }
    Invoke-WiaServiceOperation -Operation $operation -StopMessage 'Stopping the Windows Image Acquisition service (stisvc)'
}

try {
    if (($TrustCertificate -or $UntrustCertificate) -and -not $Apply) { throw 'Certificate trust changes require -Apply' }
    if ($TrustCertificate -and $UntrustCertificate) { throw 'Choose one of -TrustCertificate / -UntrustCertificate' }
    if ($TrustCertificate) {
        if (-not (Test-Admin)) { throw 'Elevation is required to trust a machine-wide certificate' }
        if ($Action -eq 'Uninstall') { throw '-TrustCertificate does not combine with Uninstall' }
        Grant-Trust (Read-Package)
        if ($Action -eq 'Status') { exit 0 }
    }
    if ($UntrustCertificate) {
        if (-not (Test-Admin)) { throw 'Elevation is required to change machine certificate stores' }
        if ($Action -in @('Install', 'Update')) { throw '-UntrustCertificate does not combine with Install/Update' }
        if ($Action -eq 'Status') { Revoke-Trust; exit 0 }
    }

    switch ($Action) {
        'Status' { Show-Status; exit 0 }
        'Install' {
            $info = Read-Package
            Show-Status
            $installed = @(Get-ProjectPackages)
            $newer = @($installed | Where-Object Version -gt $info.Version)
            if ($newer.Count -ne 0) { throw "A newer project package ($($newer[0].Version)) is already installed; use -Action Uninstall first if you really want $($info.Version)" }
            $same = @($installed | Where-Object Version -eq $info.Version)
            $older = @($installed | Where-Object Version -lt $info.Version)
            Assert-Trusted $info
            Assert-BindingAcceptable
            if ($same.Count -ne 0 -and $older.Count -eq 0) {
                Write-Log "Package $($info.Version) is already installed; verifying"
                if (-not $Apply) { exit 0 }
                if (Get-ScannerOrNull) {
                    if (-not (Test-Admin)) { throw 'Elevation is required' }
                    if ((Get-WiaDeviceCount) -lt 1) { Restart-WiaService }
                }
                Verify-Installed $info
                Show-ScanAppHints
                exit 0
            }
            if (-not $Apply) {
                if ($older.Count -ne 0) { Write-Log "Preflight passed. Would install $($info.Version) over $(($older | ForEach-Object Version) -join ', ') and delete the superseded package(s)" }
                else { Write-Log "Preflight passed. Would run pnputil /add-driver $($info.Dir)\wc3119-wia.inf /install" }
                exit 0
            }
            if (-not (Test-Admin)) { throw 'Elevation is required' }
            Install-Package $info $older
            exit $script:installExit
        }
        'Update' {
            $info = Read-Package
            Show-Status
            $installed = @(Get-ProjectPackages)
            if ($installed.Count -eq 0) { throw 'No project package is installed; use -Action Install' }
            $newest = ($installed | Sort-Object Version -Descending)[0]
            if ($info.Version -le $newest.Version) { throw "Package version $($info.Version) is not newer than installed $($newest.Version); bump DriverVer in the INF" }
            Assert-Trusted $info
            Assert-BindingAcceptable
            if (-not $Apply) { Write-Log "Preflight passed. Would install $($info.Version) over $($newest.Version) and delete $($newest.Published)"; exit 0 }
            if (-not (Test-Admin)) { throw 'Elevation is required' }
            Install-Package $info $installed
            exit $script:installExit
        }
        'Uninstall' {
            Show-Status
            $installed = @(Get-ProjectPackages)
            if ($installed.Count -eq 0 -and -not (Test-Path "Registry::HKEY_CLASSES_ROOT\CLSID\$driverClsid")) {
                Write-Log 'Nothing to uninstall'
                if ($UntrustCertificate -and $Apply) { Revoke-Trust }
                exit 0
            }
            if (-not $Apply) { Write-Log ("Preflight passed. Would delete {0} and the CLSID key; MI_00 returns to no driver (problem 28) until re-paired" -f (($installed | ForEach-Object Published) -join ', ')); exit 0 }
            if (-not (Test-Admin)) { throw 'Elevation is required' }
            Invoke-DriverUninstall $installed
            exit 0
        }
    }
} catch {
    Write-Log "FAILED: $($_.Exception.Message)"
    exit 1
}

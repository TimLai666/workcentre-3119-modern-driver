$ErrorActionPreference = 'Stop'

$setupPath = Join-Path $PSScriptRoot '..\wc3119-setup.ps1'
$tokens = $null
$parseErrors = $null
$setupAst = [System.Management.Automation.Language.Parser]::ParseFile(
    (Resolve-Path $setupPath), [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count -ne 0) { throw "Setup script parse failed: $($parseErrors[0].Message)" }

# Load only selected function definitions. The setup script's parameter block,
# main try/switch, elevation checks, and exit statements are never evaluated.
foreach ($name in @('Stop-WiaService', 'Restore-WiaServiceState', 'Invoke-WiaServiceOperation', 'Restart-WiaService', 'Install-Package', 'Invoke-DriverUninstall')) {
    $definition = $setupAst.FindAll({
        param($node)
        $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name
    }, $true) | Select-Object -First 1
    if ($definition) { . ([scriptblock]::Create($definition.Extent.Text)) }
}

$script:Harness = $null
$script:driverClsid = '{F71A8435-0000-0000-0000-000000000000}'
$script:scannerPattern = 'VID_0924&PID_4265&MI_00'
$script:imageClass = '{6BDD1FC6-810F-11D0-BEC7-08002BE2092F}'
$script:UntrustCertificate = $false

function Reset-Harness([string]$Status = 'Running') {
    $script:Harness = [ordered]@{
        Status = $Status
        ScannerPresent = $true
        StopCount = 0
        StartCount = 0
        RestartCount = 0
        PnpCallCount = 0
        PnpCode = 0
        FailPnpOnCall = 0
        FailStart = $false
        FailStartOnCall = 0
        FailRestart = $false
        FailProtectedCheck = $false
        Logs = [System.Collections.Generic.List[string]]::new()
    }
}

function Assert-Equal($Expected, $Actual, [string]$Message) {
    if ($Expected -ne $Actual) { throw "$Message (expected '$Expected', got '$Actual')" }
}
function Assert-Prefix([string]$Expected, [string]$Actual, [string]$Message) {
    if (-not $Actual.StartsWith($Expected, [System.StringComparison]::Ordinal)) { throw "$Message (expected prefix '$Expected', got '$Actual')" }
}

function Invoke-ExpectFailure([scriptblock]$Action, [string]$ExpectedMessage) {
    $actualError = $null
    try { & $Action 2>$null } catch { $actualError = $_ }
    if ($null -eq $actualError) { throw "Expected failure '$ExpectedMessage', but operation succeeded" }
    if ($actualError.Exception.Message -notlike "*$ExpectedMessage*") { throw "Expected original failure '$ExpectedMessage', got '$($actualError.Exception.Message)'" }
    return $actualError
}

function Get-Service {
    [CmdletBinding()]
    param([string]$Name)
    [pscustomobject]@{ Status = $script:Harness.Status }
}

function Stop-Service {
    [CmdletBinding()]
    param([string]$Name, [switch]$Force)
    $script:Harness.StopCount++
    $script:Harness.Status = 'Stopped'
}

function Start-Service {
    [CmdletBinding()]
    param([string]$Name)
    $script:Harness.StartCount++
    if ($script:Harness.FailStart -or $script:Harness.FailStartOnCall -eq $script:Harness.StartCount) {
        $record = [System.Management.Automation.ErrorRecord]::new(
            [System.InvalidOperationException]::new('mock start failure'),
            'MockStartFailure',
            [System.Management.Automation.ErrorCategory]::InvalidOperation,
            'service-target')
        $PSCmdlet.ThrowTerminatingError($record)
    }
    $script:Harness.Status = 'Running'
}
function Restart-Service {
    [CmdletBinding()]
    param([string]$Name, [switch]$Force)
    $script:Harness.RestartCount++
    if ($script:Harness.FailRestart) {
        $script:Harness.Status = 'Stopped'
        $record = [System.Management.Automation.ErrorRecord]::new(
            [System.InvalidOperationException]::new('mock restart failure'),
            'MockRestartFailure',
            [System.Management.Automation.ErrorCategory]::InvalidOperation,
            'restart-target')
        $PSCmdlet.ThrowTerminatingError($record)
    }
    $script:Harness.Status = 'Running'
}

function Write-Log([string]$Text) { $script:Harness.Logs.Add($Text) }
function Save-Backup {}
function Get-ScannerOrNull {
    if ($script:Harness.ScannerPresent) { [pscustomobject]@{ InstanceId = 'mock-scanner' } }
}
function Invoke-Pnputil {
    [CmdletBinding()]
    param([string[]]$Arguments)
    $script:Harness.PnpCallCount++
    if ($script:Harness.FailPnpOnCall -eq $script:Harness.PnpCallCount) {
        $record = [System.Management.Automation.ErrorRecord]::new(
            [System.InvalidOperationException]::new('mock pnputil failure'),
            'MockPnpFailure',
            [System.Management.Automation.ErrorCategory]::InvalidOperation,
            'pnp-target')
        $PSCmdlet.ThrowTerminatingError($record)
    }
    $script:Harness.PnpCode
}
function Assert-ProtectedUnchanged {
    if ($script:Harness.FailProtectedCheck) { throw 'mock protected-device failure' }
}
function Complete-PendingDeviceChange { $false }
function Verify-Installed($Expected) {}
function Show-ScanAppHints {}
function Get-ProjectPackages { @() }
function Get-WiaDeviceCount { 1 }
function Get-PnpDevice { @() }
function Start-Sleep {}
function Test-Path([string]$LiteralPath) { $false }
function Remove-Item([string]$LiteralPath, [switch]$Recurse) {}
function Get-Properties([string]$Id) { @{} }
function Revoke-Trust {}

$failures = [System.Collections.Generic.List[string]]::new()
$passed = 0
function Run-Test([string]$Name, [scriptblock]$Test) {
    try {
        & $Test
        $script:passed++
        Write-Host "PASS $Name"
    } catch {
        $script:failures.Add("$Name`: $($_.Exception.Message)")
        Write-Host "FAIL $Name`: $($_.Exception.Message)"
    }
}

$info = [pscustomobject]@{ Dir = 'mock-package'; Version = '0.2.18.0' }
$oldPackages = @()

Run-Test 'running service is restored after install pnputil failure' {
    Reset-Harness 'Running'; $script:Harness.FailPnpOnCall = 1
    Invoke-ExpectFailure { Install-Package $info $oldPackages } 'mock pnputil failure'
    Assert-Equal 'Running' $script:Harness.Status 'stisvc state'
    Assert-Equal 1 $script:Harness.StartCount 'recovery start count'
}
Run-Test 'stopped service stays stopped after install failure' {
    Reset-Harness 'Stopped'; $script:Harness.FailPnpOnCall = 1
    Invoke-ExpectFailure { Install-Package $info $oldPackages } 'mock pnputil failure'
    Assert-Equal 'Stopped' $script:Harness.Status 'stisvc state'
    Assert-Equal 0 $script:Harness.StartCount 'recovery start count'
}
Run-Test 'pending service state is rejected before stopping the service' {
    Reset-Harness 'StartPending'
    Invoke-ExpectFailure { Install-Package $info $oldPackages } 'StartPending'
    Assert-Equal 'StartPending' $script:Harness.Status 'stisvc state'
    Assert-Equal 0 $script:Harness.StopCount 'service stop count'
}
Run-Test 'protected-device failure restores the prior service state' {
    Reset-Harness 'Running'; $script:Harness.FailProtectedCheck = $true
    Invoke-ExpectFailure { Install-Package $info $oldPackages } 'mock protected-device failure'
    Assert-Equal 'Running' $script:Harness.Status 'stisvc state'
}
Run-Test 'recovery failure is logged without replacing the install error' {
    Reset-Harness 'Running'; $script:Harness.FailPnpOnCall = 1; $script:Harness.FailStart = $true
    $failure = Invoke-ExpectFailure { Install-Package $info $oldPackages } 'mock pnputil failure'
    Assert-Prefix 'MockPnpFailure' $failure.FullyQualifiedErrorId 'original error id'
    Assert-Equal 'pnp-target' $failure.TargetObject 'original error target'
    if (-not (@($script:Harness.Logs) -match 'mock start failure')) { throw 'secondary recovery error was not logged' }
}
Run-Test 'failed direct WIA restart restores the running service' {
    Reset-Harness 'Running'; $script:Harness.FailRestart = $true
    Invoke-ExpectFailure { Restart-WiaService } 'mock restart failure'
    Assert-Equal 'Running' $script:Harness.Status 'stisvc state'
    Assert-Equal 1 $script:Harness.StartCount 'recovery start count'
}
Run-Test 'successful install keeps its single intended WIA restart' {
    Reset-Harness 'Running'
    Install-Package $info $oldPackages
    Assert-Equal 0 $script:installExit 'install exit code'
    Assert-Equal 1 $script:Harness.RestartCount 'WIA restart count'
    Assert-Equal 0 $script:Harness.StartCount 'additional recovery start count'
}
Run-Test '3010 install path retains its reboot-required result' {
    Reset-Harness 'Running'; $script:Harness.PnpCode = 3010
    Install-Package $info $oldPackages
    Assert-Equal 3010 $script:installExit 'install exit code'
    Assert-Equal 0 $script:Harness.RestartCount 'WIA restart count'
    Assert-Equal 'Running' $script:Harness.Status 'stisvc state'
}
Run-Test '3010 service-start failure is reported and recovers the original state' {
    Reset-Harness 'Running'; $script:Harness.PnpCode = 3010; $script:Harness.FailStartOnCall = 1
    $failure = Invoke-ExpectFailure { Install-Package $info $oldPackages } 'mock start failure'
    Assert-Prefix 'MockStartFailure' $failure.FullyQualifiedErrorId 'original error id'
    Assert-Equal 'service-target' $failure.TargetObject 'original error target'
    Assert-Equal 'Running' $script:Harness.Status 'stisvc state'
    Assert-Equal 2 $script:Harness.StartCount 'attempt and recovery start count'
}
Run-Test 'install without a scanner does not stop or restart stisvc' {
    Reset-Harness 'Stopped'; $script:Harness.ScannerPresent = $false
    Install-Package $info $oldPackages
    Assert-Equal 0 $script:installExit 'install exit code'
    Assert-Equal 'Stopped' $script:Harness.Status 'stisvc state'
    Assert-Equal 0 $script:Harness.StopCount 'service stop count'
    Assert-Equal 0 $script:Harness.RestartCount 'WIA restart count'
}
Run-Test 'uninstall pnputil failure restores the running service' {
    Reset-Harness 'Running'; $script:Harness.FailPnpOnCall = 1
    $installed = @([pscustomobject]@{ Published = 'oem1.inf'; Version = '0.2.17.0' })
    Invoke-ExpectFailure { Invoke-DriverUninstall $installed } 'mock pnputil failure'
    Assert-Equal 'Running' $script:Harness.Status 'stisvc state'
    Assert-Equal 1 $script:Harness.StartCount 'recovery start count'
}
Run-Test 'uninstall failure after service start restores an initially stopped service' {
    Reset-Harness 'Stopped'; $script:Harness.FailProtectedCheck = $true
    $installed = @([pscustomobject]@{ Published = 'oem1.inf'; Version = '0.2.17.0' })
    Invoke-ExpectFailure { Invoke-DriverUninstall $installed } 'mock protected-device failure'
    Assert-Equal 'Stopped' $script:Harness.Status 'stisvc state'
}
Run-Test 'uninstall service-start failure is reported and recovers the original state' {
    Reset-Harness 'Running'; $script:Harness.FailStartOnCall = 1
    $installed = @([pscustomobject]@{ Published = 'oem1.inf'; Version = '0.2.17.0' })
    $failure = Invoke-ExpectFailure { Invoke-DriverUninstall $installed } 'mock start failure'
    Assert-Prefix 'MockStartFailure' $failure.FullyQualifiedErrorId 'original error id'
    Assert-Equal 'Running' $script:Harness.Status 'stisvc state'
    Assert-Equal 2 $script:Harness.StartCount 'attempt and recovery start count'
}

Write-Host "Passed: $passed; failed: $($failures.Count)"
if ($failures.Count -ne 0) { exit 1 }

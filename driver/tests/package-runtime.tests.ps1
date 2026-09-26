$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$packageScript = Join-Path $projectRoot 'driver/package.ps1'
$tokens = $null
$parseErrors = $null
$packageAst = [System.Management.Automation.Language.Parser]::ParseFile($packageScript, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count -gt 0) { throw "Could not parse package.ps1: $($parseErrors[0].Message)" }
$guardAsts = @($packageAst.FindAll({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq 'Assert-NoVisualCppRuntimeImports'
}, $true))
if ($guardAsts.Count -ne 1) { throw 'package.ps1 must define exactly one Assert-NoVisualCppRuntimeImports function.' }
. ([scriptblock]::Create($guardAsts[0].Extent.Text))

$testRoot = [IO.Path]::GetFullPath((Join-Path ([IO.Path]::GetTempPath()) ('wc3119-runtime-unit-' + [Guid]::NewGuid().ToString('N'))))
$tempPrefix = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([char[]]@('\', '/')) + [IO.Path]::DirectorySeparatorChar
if (-not $testRoot.StartsWith($tempPrefix, [StringComparison]::OrdinalIgnoreCase)) { throw "Unsafe test output path: $testRoot" }
New-Item -ItemType Directory -Path $testRoot | Out-Null

function New-MockDumpbin {
    param([string]$Name, [string[]]$Dependencies, [int]$ExitCode)

    $path = Join-Path $testRoot ($Name + '.cmd')
    $lines = @('@echo off')
    if ($Dependencies.Count -gt 0) {
        $lines += 'echo   Image has the following dependencies:'
        $lines += 'echo'
        foreach ($dependency in $Dependencies) { $lines += ('echo     ' + $dependency) }
    }
    $lines += ('exit /b ' + $ExitCode)
    [IO.File]::WriteAllLines($path, [string[]]$lines, [Text.Encoding]::ASCII)
    return $path
}

function Assert-Rejected {
    param([string]$Name, [string]$ExpectedText)

    $rejected = $false
    $errorMessage = $null
    try {
        Assert-NoVisualCppRuntimeImports -Path (Join-Path $testRoot 'fixture.dll')
    } catch {
        $rejected = $true
        $errorMessage = $_.Exception.Message
    }
    if (-not $rejected) { throw "$Name was accepted unexpectedly." }
    if ($errorMessage -notmatch [regex]::Escape($ExpectedText)) { throw "$Name was rejected for an unexpected reason: $errorMessage" }
}

try {
    $DumpbinPath = New-MockDumpbin -Name 'allowed-system' -Dependencies @(
        'KERNEL32.dll', 'ntdll.dll', 'msvcrt.dll', 'msvcp_win.dll', 'ucrtbase.dll', 'api-ms-win-crt-runtime-l1-1-0.dll'
    ) -ExitCode 0
    Assert-NoVisualCppRuntimeImports -Path (Join-Path $testRoot 'fixture.dll')

    $DumpbinPath = New-MockDumpbin -Name 'reject-vcruntime' -Dependencies @('KERNEL32.dll', 'VCRUNTIME140.dll') -ExitCode 0
    Assert-Rejected -Name 'VCRUNTIME140.dll' -ExpectedText 'VCRUNTIME140.dll'

    $DumpbinPath = New-MockDumpbin -Name 'reject-msvcp' -Dependencies @('MSVCP140_1.dll') -ExitCode 0
    Assert-Rejected -Name 'MSVCP140_1.dll' -ExpectedText 'MSVCP140_1.dll'

    $DumpbinPath = New-MockDumpbin -Name 'reject-tool-error' -Dependencies @('KERNEL32.dll') -ExitCode 17
    Assert-Rejected -Name 'dumpbin nonzero exit' -ExpectedText 'exit code 17'

    $DumpbinPath = New-MockDumpbin -Name 'reject-empty-output' -Dependencies @() -ExitCode 0
    Assert-Rejected -Name 'empty dumpbin output' -ExpectedText 'no DLL dependency names'

    Write-Output 'PASS: system DLLs are allowed; VCRUNTIME/MSVCP imports, dumpbin errors, and empty output are rejected.'
} finally {
    if (Test-Path -LiteralPath $testRoot) { Remove-Item -LiteralPath $testRoot -Recurse -Force }
}

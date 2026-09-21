# Run from any caller directory with Windows PowerShell 5.1 or newer.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
# Native tools use stderr for diagnostics too; their exit code is authoritative.
$PSNativeCommandUseErrorActionPreference = $false

function Assert-WindowsX64 {
    param(
        [string]$Platform = [Environment]::OSVersion.Platform,
        [string]$OSArchitecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture,
        [string]$ProcessArchitecture = [System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture
    )
    if ($Platform -ne 'Win32NT' -or $OSArchitecture -ne 'X64' -or $ProcessArchitecture -ne 'X64') {
        throw 'Verification requires native Windows x64 and a 64-bit PowerShell process.'
    }
}

function Invoke-CheckedNative {
    param([string]$Command, [string[]]$Arguments)
    Write-Host ('=> ' + $Command + ' ' + ($Arguments -join ' '))
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        $failure = New-Object System.Exception("$Command failed with exit code $LASTEXITCODE")
        $failure.Data['ExitCode'] = $LASTEXITCODE
        throw $failure
    }
}

$verificationExit = 1
$verificationStage = 'host validation'
$locationPushed = $false
try {
    Assert-WindowsX64
    $repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
    Push-Location -LiteralPath $repositoryRoot
    $locationPushed = $true
    $packageManager = (Get-Content -Raw -LiteralPath 'app/package.json' | ConvertFrom-Json).packageManager
    if ($packageManager -notmatch '^pnpm@\d+\.\d+\.\d+$') {
        throw 'app/package.json must pin an exact pnpm version.'
    }

    $verificationStage = 'Rust formatting'
    Invoke-CheckedNative 'cargo' @('fmt', '--all', '--', '--check')
    # Explicit Corepack selection also works when the outer directory defaults to another pnpm.
    $verificationStage = 'frontend dependency installation'
    Invoke-CheckedNative 'corepack' @($packageManager, '--dir', 'app', 'install', '--frozen-lockfile')
    # Tauri tests need these ignored resources even on a fresh checkout.
    $verificationStage = 'Edge resource preparation'
    Invoke-CheckedNative 'node' @('scripts/prepare-edge-bundle.mjs')
    # Native fixtures share the current user's desktop, clipboard and registry session. Keep each
    # package visible as its own CI stage so a hosted-run failure is actionable without private logs.
    foreach ($rustPackage in @(
        'autologin-core',
        'platform-macos',
        'platform-runtime',
        'autologin-native-host',
        'autologin-desktop'
    )) {
        $verificationStage = "Rust tests: $rustPackage"
        Invoke-CheckedNative 'cargo' @('test', '--locked', '-p', $rustPackage, '--all-targets', '--', '--test-threads=1')
    }
    # Keep every Windows-native test binary separate. Besides making the public annotation useful,
    # this prevents a failure in one desktop integration from hiding the remaining test boundary.
    $verificationStage = 'Rust tests: platform-windows library'
    Invoke-CheckedNative 'cargo' @('test', '--locked', '-p', 'platform-windows', '--lib', '--', '--test-threads=1')
    foreach ($testTarget in @(
        'catalog_native',
        'clipboard_native',
        'credentials_native',
        'edge_install',
        'launch_native',
        'test_boundaries'
    )) {
        $verificationStage = "Rust tests: platform-windows/$testTarget"
        Invoke-CheckedNative 'cargo' @('test', '--locked', '-p', 'platform-windows', '--test', $testTarget, '--', '--test-threads=1')
    }
    $verificationStage = 'script regression tests'
    Invoke-CheckedNative 'node' @('--test', 'scripts/bundle-native-host.test.mjs', 'scripts/tauri.test.mjs', 'scripts/verify-windows.test.mjs')
    $verificationStage = 'frontend type checking'
    Invoke-CheckedNative 'corepack' @($packageManager, '--dir', 'app', 'typecheck')
    $verificationStage = 'frontend tests'
    Invoke-CheckedNative 'corepack' @($packageManager, '--dir', 'app', 'test', '--', '--run')
    $verificationStage = 'frontend production build'
    Invoke-CheckedNative 'corepack' @($packageManager, '--dir', 'app', 'build')
    $verificationStage = 'Tauri debug bundle build'
    Invoke-CheckedNative 'corepack' @($packageManager, '--dir', 'app', 'tauri', 'build', '--debug')
    $verificationExit = 0
    Write-Host 'Windows foundation verification passed. Bundles were built, not installed.'
}
catch {
    if ($_.Exception.Data.Contains('ExitCode')) {
        $verificationExit = [int]$_.Exception.Data['ExitCode']
    }
    if ($env:GITHUB_ACTIONS -eq 'true') {
        $safeMessage = $_.Exception.Message.Replace("`r", ' ').Replace("`n", ' ').Replace('%', '%25')
        Write-Host "::error title=Windows verification failed::$verificationStage - $safeMessage"
    }
    Write-Error -Message $_.Exception.Message -ErrorAction Continue
}
finally {
    if ($locationPushed) { Pop-Location }
}
exit $verificationExit

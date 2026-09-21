import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const script = fileURLToPath(new URL('./verify-windows.ps1', import.meta.url));
const windows = process.platform === 'win32';
const quote = (value) => `'${value.replaceAll("'", "''")}'`;
function powershell(source, options = {}) {
  return spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
    '-EncodedCommand', Buffer.from(source, 'utf16le').toString('base64')], {
    encoding: 'utf8', timeout: 30_000, windowsHide: true, ...options,
  });
}

function fixture(t, failAt) {
  const temporary = mkdtempSync(path.join(os.tmpdir(), 'logindeck-verify-'));
  t.after(() => rmSync(temporary, { recursive: true, force: true }));
  const root = path.join(temporary, "nested repository & quote's");
  const caller = path.join(temporary, 'unrelated caller');
  mkdirSync(path.join(root, 'scripts'), { recursive: true });
  mkdirSync(path.join(root, 'app'));
  mkdirSync(caller);
  copyFileSync(script, path.join(root, 'scripts', 'verify-windows.ps1'));
  writeFileSync(path.join(root, 'app', 'package.json'), JSON.stringify({ packageManager: 'pnpm@11.19.0' }));
  const trace = path.join(temporary, 'trace.jsonl');
  const native = path.join(temporary, 'native.cjs');
  writeFileSync(native, `
    const fs = require('node:fs');
    const call = { cwd: process.cwd(), tool: process.argv[2], args: process.argv.slice(3) };
    fs.appendFileSync(process.env.VERIFY_TRACE, JSON.stringify(call) + '\\n');
    const count = fs.readFileSync(process.env.VERIFY_TRACE, 'utf8').trim().split('\\n').length;
    // Real native stderr on success must not be mistaken for failure in Windows PowerShell 5.1.
    console.error('fixture native diagnostic');
    process.exit(count === Number(process.env.VERIFY_FAIL_AT) ? 37 : 0);
  `);
  const result = powershell(`
    function Invoke-Fixture {
      & ${quote(process.execPath)} ${quote(native)} @args
      $global:LASTEXITCODE = $LASTEXITCODE
    }
    function cargo { Invoke-Fixture 'cargo' @args }
    function corepack { Invoke-Fixture 'corepack' @args }
    function node { Invoke-Fixture 'node' @args }
    & ${quote(path.join(root, 'scripts', 'verify-windows.ps1'))}
    $result = $LASTEXITCODE
    Write-Output ('caller-cwd:' + (Get-Location).Path)
    exit $result
  `, { cwd: caller, env: { ...process.env, VERIFY_TRACE: trace, VERIFY_FAIL_AT: String(failAt ?? 0) } });
  const calls = readFileSync(trace, 'utf8').trim().split('\n').map(JSON.parse);
  return { result, calls, root, caller };
}

const expectedCalls = [
  ['cargo', 'fmt', '--all', '--', '--check'],
  ['corepack', 'pnpm@11.19.0', '--dir', 'app', 'install', '--frozen-lockfile'],
  ['node', 'scripts/prepare-edge-bundle.mjs'],
  ...[
    'autologin-core',
    'platform-macos',
    'platform-windows',
    'platform-runtime',
    'autologin-native-host',
    'autologin-desktop',
  ].map((name) => ['cargo', 'test', '--locked', '-p', name, '--all-targets', '--', '--test-threads=1']),
  ['node', '--test', 'scripts/bundle-native-host.test.mjs', 'scripts/tauri.test.mjs', 'scripts/verify-windows.test.mjs'],
  ['corepack', 'pnpm@11.19.0', '--dir', 'app', 'typecheck'],
  ['corepack', 'pnpm@11.19.0', '--dir', 'app', 'test', '--', '--run'],
  ['corepack', 'pnpm@11.19.0', '--dir', 'app', 'build'],
  ['corepack', 'pnpm@11.19.0', '--dir', 'app', 'tauri', 'build', '--debug'],
];

test('Windows verifier resolves its own root, uses the package pin and prepares clean-checkout resources', { skip: !windows }, (t) => {
  const { result, calls, root, caller } = fixture(t);
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
  assert.ok(calls.every((call) => call.cwd === root), JSON.stringify(calls));
  assert.deepEqual(calls.map(({ tool, args }) => [tool, ...args]), expectedCalls);
  assert.ok(result.stdout.includes(`caller-cwd:${caller}`), result.stdout);
});

test('Windows verifier stops at each failed native stage and preserves its exit code', { skip: !windows }, (t) => {
  for (let stage = 1; stage <= expectedCalls.length; stage += 1) {
    const { result, calls, caller } = fixture(t, stage);
    assert.equal(result.status, 37, `stage ${stage}: ${result.stdout}\n${result.stderr}`);
    assert.equal(calls.length, stage);
    assert.ok(result.stdout.includes(`caller-cwd:${caller}`), result.stdout);
  }
});

test('Windows verifier host guard rejects Unix, x86 and ARM64 including x64 emulation', { skip: !windows }, () => {
  // Extract the real guard without running build commands or introducing a production bypass.
  readFileSync(script);
  const result = powershell(`
    $ErrorActionPreference = 'Stop'
    $tokens = $null; $errors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseFile(${quote(script)}, [ref]$tokens, [ref]$errors)
    if ($errors.Count -ne 0) { throw 'PowerShell parse failed' }
    $guard = $ast.Find({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Assert-WindowsX64' }, $true)
    if ($null -eq $guard) { throw 'Missing host guard' }
    . ([scriptblock]::Create($guard.Extent.Text))
    Assert-WindowsX64 -Platform Win32NT -OSArchitecture X64 -ProcessArchitecture X64
    foreach ($case in @(@('Unix', 'X64', 'X64'), @('Win32NT', 'X86', 'X86'), @('Win32NT', 'Arm64', 'X64'), @('Win32NT', 'X64', 'X86'))) {
      $rejected = $false
      try { Assert-WindowsX64 -Platform $case[0] -OSArchitecture $case[1] -ProcessArchitecture $case[2] }
      catch { $rejected = $true; if ($_.Exception.Message -notmatch 'Windows x64') { throw } }
      if (-not $rejected) { throw ('Accepted unsupported host: ' + $case) }
    }
  `);
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
});

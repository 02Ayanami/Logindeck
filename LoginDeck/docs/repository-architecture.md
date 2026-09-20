# LoginDeck repository architecture

LoginDeck shares its React frontend, Tauri commands, SQLite schema, domain services and native-host
protocol across macOS and Windows. The Windows foundation is implemented and locally built on
Windows 11 x64. Windows 10 22H2 is a supported target awaiting native acceptance; this milestone
does not establish full macOS feature parity. See the [evidence and limitations](testing/2026-09-18-windows-foundation.md).

## Layers

```text
app/                         Shared React UI and Tauri desktop composition
crates/autologin-core/       Platform-neutral domain, repositories, services and contracts
crates/platform-runtime/     Compile-time platform selection and stable native facade
crates/platform-macos/       Keychain, clipboard, app catalog, icons; historical login code
crates/platform-windows/     Credential Manager, clipboard, Win32/MSIX catalog, icons and launch
crates/native-host/          Shared Edge native-messaging process
browser-extension/           Shared Edge extension
```

Dependency direction is one way:

```text
frontend -> desktop commands -> autologin-core interfaces
                         \-> platform-runtime -> platform-macos   (macOS)
native-host ----------------^                 -> platform-windows (Windows)
```

`autologin-core` has no Tauri or concrete operating-system dependency. Product composition imports
native types through `platform-runtime`; all Windows APIs stay inside `platform-windows`.
Target-specific Cargo dependencies select the actual backend. Other runtime operating systems
fail compilation explicitly.

## Runtime and product boundary

Both runtime branches expose credential, clipboard and catalog aliases, icon facades, backend status
and a stable platform value. Windows uses `windows` / `windows_credential_manager`; macOS retains
`macos` / `login_keychain`. SQLite stores secret references, with existing recovery services handling
interrupted changes across the system credential store and database. Migration SQL is pinned to LF
in `.gitattributes` because SQLx checksums cover its exact embedded bytes.

Windows discovery combines HKCU/HKLM uninstall records in both registry views with current-user
package Application entries. Start Menu shortcuts and bounded installation-directory searches help
resolve launchable Win32 targets; there is no whole-disk scan. Persisted targets are explicitly
`exe:<absolute .exe path>` or `aumid:<AUMID>`. Import uses the same validation and reuses known Win32
identities. Saved icons use the complete trusted record; package scan previews currently use a
placeholder, while saved package records can resolve a native logo.

Launch rechecks the record and target before explicit `CreateProcessW` or AUMID activation. Win32
fingerprints contain canonical path, volume/file identity, length and modification time; package
fingerprints contain registration full name and AUMID. These are metadata consistency checks,
not content hashes, Authenticode validation or publisher trust. File guards narrow replacement
races but verification and native process creation are not an OS-level atomic transaction.
Credential creation is serialized across cooperating in-process callers; Credential Manager has
no atomic cross-process create-if-absent primitive.

The shared desktop command surface remains bounded to account management, copy, explicit open,
scan/import, settings and existing browser support. Historical macOS login modules are not
registered as current product commands. Windows has no automatic-login facade, UI Automation,
automatic fill or account switching. Building Edge resources does not enable Windows native
messaging by itself; the Settings command installs the fixed per-user host files and registers only
`HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.autologin.native`. Store publication,
machine-wide registration and automatic extension installation remain outside this milestone.

## Verification

macOS keeps `./scripts/verify-macos.sh` and its existing workflow definition. That definition lives
under `LoginDeck/.github/workflows` in this outer Git checkout and is not relocated by this task;
GitHub only discovers workflows in the outer root `.github/workflows`. Windows requires Windows 10 22H2
or Windows 11 x64, native x64 PowerShell 5.1+, Rust 1.89.0 MSVC with rustfmt, Visual Studio 2022
C++ build tools and Windows SDK, WebView2 Runtime, Node.js 22 and Corepack/pnpm 11.19.0.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-windows.ps1
```

The script resolves the nested product root relative to itself, honors the application package-manager
pin, prepares ignored resources for clean checkouts, runs every workspace target and native test
serially, runs script/frontend checks and creates the full Tauri debug bundles. Native failures stop
verification with their exit code. No interactive native test is silently removed for CI.

Windows CI lives at the outer Git root `.github/workflows/windows.yml` with `LoginDeck` as its
working directory. It is added only after a successful local canonical run, uses `windows-2022`, Node 22,
pnpm 11.19.0 and Rust 1.89.0, and runs this same script. Hosted CI execution is unverified until an
actual remote run; Windows Server runner results do not replace Windows 10/11 desktop acceptance.
Native clipboard tests use the current user's clipboard session. Controlled package activation
requires an explicitly installed fixture and remains separately ignored/unverified when absent.
No installers are executed by verification. Current manual UI, package logo/activation, Windows 10
and macOS-native gaps are recorded explicitly in the testing report.

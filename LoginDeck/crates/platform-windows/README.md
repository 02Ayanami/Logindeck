# LoginDeck Windows platform

This workspace crate implements the shared `CredentialStore`, `Clipboard` and `ApplicationCatalog`
contracts and is selected by `platform-runtime` on Windows. All Win32, COM and WinRT calls stay
here; the frontend, desktop commands and domain services remain shared.

## Implemented scope

- Credential Manager generic entries in `LoginDeck/<service>/<uuid>`, bounded UTF-8 blobs,
  no overwrite for cooperating in-process callers, zeroed temporary buffers and sanitized errors.
- Unicode clipboard writes, bounded lock retry and sequence-token checks before and after locking
  for conditional cleanup. The shared service owns the existing 30-second timer.
- Launchable Win32 inventory from HKCU/HKLM, 32/64-bit uninstall views, assisted by bounded Start
  Menu and install-root resolution. No full-disk traversal or execution of uninstall commands.
- Current-user MSIX/UWP Application inventory, stable typed targets, deterministic merge and
  `.exe`/bounded-directory manual import. Registered imports reuse the automatic identity.
- Win32 resource icons and saved-package logo decoding with bounded PNG output and safe `None`
  fallback. Package scan previews retain a placeholder until a record is saved.
- Record/target revalidation before explicit Win32 process creation or AUMID activation, with real
  PID propagation and no shell interpretation.

`WindowsLaunchTarget` persists `exe:<absolute .exe path>` or `aumid:<AUMID>`; package identifiers
are never treated as filesystem paths. `signature_identity` stores metadata: path, volume/file ID,
size and last-write time for Win32; full registered package name and AUMID for packages. It is not
an Authenticode signature, content authentication or publisher trust assertion. Ancestor handles
inspect metadata and reject reparse points; metadata-only access does not exclude mutation. The
retained final read-data handle disallows ordinary write/delete sharing during process creation,
but verification and launch are not a system-level atomic transaction. Credential Manager also has
no cross-process atomic create-if-absent guarantee against a non-cooperating external writer.

Windows automatic login, UI Automation, automatic filling, account switching, uninstall/repair/update,
and Edge native-messaging installation/capture are outside this foundation.

## Build and validation

Supported targets: Windows 10 22H2 and Windows 11 x64 only. Use native x64 PowerShell 5.1+, Rust
1.89.0 (`x86_64-pc-windows-msvc`, rustfmt), Visual Studio 2022 C++ build tools and Windows SDK,
WebView2 Runtime, Node.js 22, Corepack and the application-pinned pnpm 11.19.0. First-time builds
need access to package registries and the Tauri WiX/NSIS downloads.

From the product repository root (the directory containing `Cargo.toml`):

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-windows.ps1
```

An absolute script path works from any caller directory. It performs the full workspace, frontend,
script and Tauri debug bundle checks; errors preserve the native exit code. The script does not
install or publish bundles. For a focused native regression run:

```powershell
cargo test -p platform-windows -- --test-threads=1
```

Native tests use owned unique credential/registry/file fixtures and controlled clipboard writes in
the current user's desktop session. The controlled Win32 launch harness verifies its child PID and
arguments. `controlled_package_activation_opt_in` is intentionally ignored unless an explicitly
installed `LoginDeck.ActivationFixture_*` package and `LOGINDECK_TEST_AUMID` are supplied; ordinary
installed applications are not a substitute for that fixture.

The crate remains a workspace member on macOS. Tests requiring Windows absolute-path, parent or
file-name semantics are gated with `cfg(windows)`, as are native API tests. Portable AUMID, text,
UTF-16 and error tests still run there. `tests/test_boundaries.rs` inspects Rust module/crate/function
cfg attributes and checks the macOS-selected tests against a reviewed portable list, including the
inert non-Windows custom launch harness. New portable tests must update that list after review.
This source-selection check also runs on Windows; it does not replace native macOS execution.

The current [verification record](../../docs/testing/2026-09-18-windows-foundation.md) covers a real
Windows 11 x64 run, EXE/MSI/NSIS build evidence and the responsive-window startup regression.
It does not claim live account/copy UI acceptance, installed-package logos, controlled MSIX
activation, installer execution, Windows 10 execution or native macOS verification.

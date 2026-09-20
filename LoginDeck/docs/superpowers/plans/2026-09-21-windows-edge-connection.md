# Windows Edge Connection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the bundled Edge extension connect to LoginDeck on Windows through an automatically installed, current-user Native Messaging host.

**Architecture:** Reuse the existing extension protocol, native-host service, SQLite schema, and Windows Credential Manager. Add fixed-path Windows installation and registry code inside `platform-windows`, expose only the application data path through `platform-runtime`, and keep the existing three Tauri Edge commands unchanged.

**Tech Stack:** Rust 1.89, windows-rs 0.61.3, Tauri 2, Microsoft Edge Manifest V3 Native Messaging, React 19.

**Spec:** `docs/superpowers/specs/2026-09-21-windows-mac-settings-parity-design.md`

## Global Constraints

- Register only `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.autologin.native`.
- Permit only the repository-pinned extension origin.
- Never accept a frontend path, executable, registry key, host name, database path, or credential namespace.
- Keep macOS behavior unchanged and keep the exact 30-command desktop allowlist.
- Do not add automatic login, UI Automation, account switching, signing, or release upload work.
- Reject reparse points and foreign registration values; never overwrite unrelated state.

## Review Focus

- A foreign value at the owned Edge key remains untouched and returns `extension.setup_conflict`.
- A junction or symbolic link in the install tree is rejected before file replacement.
- Native host and desktop resolve the same `%APPDATA%\com.autologin.desktop` database and lock path.
- The Windows host still rejects an untrusted extension-origin argument before opening storage.
- Disabling browser capture prevents subsequent candidates from being saved even while the native port exists.

---

### Task 1: Select the Shared Windows Data Directory

**Files:**
- Create: `crates/platform-windows/src/data_directory.rs`
- Modify: `crates/platform-windows/src/lib.rs`
- Create: `crates/platform-macos/src/data_directory.rs`
- Modify: `crates/platform-macos/src/lib.rs`
- Modify: `crates/platform-runtime/src/lib.rs`

**Interfaces:**
- Produces: `platform_runtime::application_data_directory() -> Result<PathBuf, AppError>`.

- [ ] Write platform tests asserting an absolute path ending in `com.autologin.desktop`; on Windows resolve `FOLDERID_RoamingAppData` with `SHGetKnownFolderPath` and free the returned memory with `CoTaskMemFree`.
- [ ] Run `cargo test -p platform-runtime -p platform-windows -p platform-macos --locked -- --test-threads=1` and confirm RED because the function is absent.
- [ ] Implement the selected function. macOS preserves `~/Library/Application Support/com.autologin.desktop`; Windows appends the fixed identifier to the known roaming-app-data folder.
- [ ] Re-run the focused tests and `cargo clippy --locked -p platform-runtime -p platform-windows -p platform-macos --all-targets -- -D warnings`.
- [ ] Commit as `feat: expose the platform LoginDeck data directory`.

---

### Task 2: Run the Existing Native Host on Windows

**Files:**
- Modify: `crates/native-host/src/main.rs`
- Create: `crates/native-host/tests/windows_startup.rs`

**Interfaces:**
- Consumes: `platform_runtime::application_data_directory()` and the existing selected `NativeCredentialStore`.
- Produces: a Windows-capable `autologin-native-host.exe` using the existing protocol.

- [ ] Add a controlled child-process test that starts the host with the pinned extension origin and sends a status request with no desktop lock; expect a framed `connected:false` response instead of an unsupported-platform exit.
- [ ] Run `cargo test -p autologin-native-host --locked -- --test-threads=1` and confirm RED on the current macOS-only guard.
- [ ] Remove only the `cfg!(target_os = "macos")` runtime rejection and replace the local HOME-based path function with the platform runtime function.
- [ ] Preserve origin validation before storage, fixed credential service `com.autologin.app`, frame bounds, session expiry, revision checks, mutation locking, and zeroization.
- [ ] Re-run native-host tests and `cargo clippy -p autologin-native-host --all-targets -- -D warnings`.
- [ ] Commit as `feat: run the native host on Windows`.

---

### Task 3: Install and Register the Windows Native Host

**Files:**
- Create: `crates/platform-windows/src/edge.rs`
- Modify: `crates/platform-windows/src/lib.rs`
- Modify: `crates/platform-windows/Cargo.toml`
- Create: `crates/platform-windows/tests/edge_install.rs`
- Modify: `crates/platform-windows/tests/test_boundaries.rs`

**Interfaces:**
- Produces:
  - `edge::install(bundled: &Path) -> Result<PathBuf, AppError>`
  - `edge::extension_path() -> Result<PathBuf, AppError>`
  - `edge::open_extensions() -> Result<(), AppError>`
  - `edge::reveal_extension() -> Result<(), AppError>`

- [ ] Add isolated tests using a unique `HKCU\Software\LoginDeck\Tests\<uuid>` fixture key and a temporary install root. Cover repeat installation, stale extension-file removal, invalid pinned identity, missing/empty `.exe`, foreign registry value preservation, staging cleanup, and reparse-point rejection.
- [ ] Run `cargo test -p platform-windows --test edge_install --locked -- --test-threads=1` and confirm RED because the module is absent.
- [ ] Validate Manifest V3 and the pinned key, copy only `extension/` plus `autologin-native-host.exe`, stage with create-new files and rename, and reject `FILE_ATTRIBUTE_REPARSE_POINT` ancestors.
- [ ] Write a fixed native-host manifest containing one absolute `.exe` path, `type: "stdio"`, and one pinned `allowed_origins` entry.
- [ ] Create/read only the fixed HKCU Edge host key in production. If its default `REG_SZ` exists with another path, return `extension.setup_conflict` without modifying it.
- [ ] Implement Edge opening and folder reveal with fixed values only; no shell or user-supplied URL/path.
- [ ] Run platform tests, formatting, and Clippy with warnings denied.
- [ ] Commit as `feat: register the Edge native host on Windows`.

---

### Task 4: Route Existing Desktop Commands on Windows

**Files:**
- Modify: `app/src-tauri/src/commands/edge.rs`
- Modify: `app/src-tauri/tests/command_contract.rs`
- Modify: `scripts/bundle-native-host.test.mjs`
- Modify: `crates/platform-windows/README.md`
- Modify: `README.md`
- Modify: `docs/repository-architecture.md`

**Interfaces:**
- Consumes: `platform_runtime::edge` on Windows and the current `edge_install` implementation on macOS.
- Preserves: `install_edge_extension`, `open_edge_extensions`, and `open_edge_extension_folder` response shapes and names.

- [ ] Add command source-contract tests proving Windows routes through `platform_runtime::edge` and the non-macOS `platform.unsupported` branch is gone; keep the exact 30-command list.
- [ ] Run the command contract test and confirm RED.
- [ ] Under `cfg(target_os = "windows")`, delegate the three commands to the new platform module inside `spawn_blocking`; under macOS retain current behavior.
- [ ] Ensure `prepare-edge-bundle.mjs` and its tests place `autologin-native-host.exe` in the bundled Edge resource directory on Windows.
- [ ] Update user documentation to state stable Edge/current-user registration support without claiming store installation or Windows 10 live verification.
- [ ] Run desktop command tests, script tests, frontend tests, typecheck, and frontend build.
- [ ] Commit as `feat: connect Edge to LoginDeck on Windows`.

---

### Task 5: End-to-End Verification

**Files:**
- Create: `docs/testing/2026-09-21-windows-edge-connection.md`

**Interfaces:**
- Produces: reproducible evidence for build, registration, connection, enable/disable, and controlled save behavior.

- [ ] Run `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-windows.ps1` and record exact pass counts.
- [ ] Launch the rebuilt desktop, open Settings, and prepare the bundled extension.
- [ ] Verify the HKCU value points to the generated manifest; parse it and verify one pinned origin and the installed `.exe` path.
- [ ] Load the unpacked extension in stable Edge, check connection, enable detection, and save one synthetic HTTPS login through the confirmation flow.
- [ ] Confirm the account appears in LoginDeck and the secret is stored under the controlled Windows Credential Manager test target; remove only the synthetic fixture afterward.
- [ ] Disable detection and verify a subsequent candidate is rejected.
- [ ] Record remaining gaps: extension-store publication, signing, installer execution, and Windows 10 live execution.
- [ ] Commit as `test: verify Windows Edge connection`.


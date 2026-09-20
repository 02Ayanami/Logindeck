# LoginDeck Windows Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver a native Windows 10 22H2 / Windows 11 x64 LoginDeck build with Credential Manager storage, guarded clipboard cleanup, launchable Win32 and MSIX/UWP discovery, icons, and verified launch.

**Architecture:** Add a `platform-windows` crate that implements the existing `autologin-core` contracts and hides all Win32, COM, and WinRT APIs. Extend `platform-runtime` with compile-time Windows selection while keeping React, Tauri commands, persistence, and product services shared. Build discovery from separate Win32 and packaged-app inventories, normalize them into one candidate type, then expose only launchable and re-verifiable applications.

**Tech Stack:** Rust 1.89, `windows` 0.61.3, `async-trait`, `secrecy`, `tokio`, `zeroize`, Tauri 2.9, React 19, PowerShell, GitHub Actions Windows runner.

**Spec:** `docs/superpowers/specs/2026-09-18-windows-foundation-design.md`

## Global Constraints

- Support Windows 10 22H2 and Windows 11 on x64 only.
- Reuse the existing React frontend, Tauri commands, SQLite schema, native host protocol, and 30-second clipboard cleanup scheduler.
- Keep every Win32, COM, and WinRT dependency inside `crates/platform-windows`.
- Read Win32 applications from HKCU/HKLM 32-bit and 64-bit uninstall inventory; use Start Menu shortcuts only to resolve launch targets.
- Read MSIX/UWP applications from the current user's package inventory.
- Return only applications with a verified `.exe` or AUMID target; never scan the whole disk.
- Do not implement Windows automatic login, UI Automation, account switching, uninstall, repair, or update behavior.
- Never log, serialize, or persist plaintext passwords outside Windows Credential Manager.
- Never clear clipboard content that replaced LoginDeck's own write.
- Do not add Windows CI until full local Windows verification succeeds.
- Implement every behavior with RED -> GREEN -> REFACTOR and observe the intended failing output first.

## File Map

Shared modifications:

- `Cargo.toml`, `Cargo.lock` — workspace membership and dependency lock.
- `crates/autologin-core/src/domain.rs` — `Platform::Windows`.
- `crates/autologin-core/tests/domain_contract.rs`, `sqlite_repository.rs` — wire and persistence contracts.
- `app/src-tauri/src/scan.rs` — deterministic Windows identity ordering.
- `crates/platform-runtime/Cargo.toml`, `src/lib.rs` — Windows target selection.
- `app/src-tauri/src/commands/applications.rs` — typed target-aware icon lookup.
- `app/src-tauri/tauri.conf.json`, `icons/icon.ico` — Windows packaging.
- `README.md`, `docs/repository-architecture.md` — verified Windows scope.

New Windows crate:

- `crates/platform-windows/Cargo.toml`, `src/lib.rs`
- `src/error.rs`, `src/target.rs`
- `src/credentials.rs`, `src/clipboard.rs`
- `src/app_catalog.rs`
- `src/app_catalog/win32.rs`, `shortcut.rs`, `package.rs`
- `src/icons.rs`, `src/launch.rs`
- `tests/credentials_native.rs`, `clipboard_native.rs`, `catalog_native.rs`
- `tests/fixtures/uninstall.json`, `packages.json`

Verification:

- `scripts/verify-windows.ps1`
- `docs/testing/2026-09-18-windows-foundation.md`
- `.github/workflows/windows.yml` only after local success.

---

### Task 1: Add Windows as a shared platform identity

**Files:**
- Modify: `crates/autologin-core/src/domain.rs:39-44`
- Modify: `crates/autologin-core/tests/domain_contract.rs`
- Modify: `crates/autologin-core/tests/sqlite_repository.rs`
- Modify: `app/src-tauri/src/scan.rs:79-87`

**Interfaces:**
- Produces `autologin_core::Platform::Windows`, serialized exactly as `"windows"`.
- Produces scan rank `Macos => 0`, `Windows => 1`.

- [ ] **Step 1: Write failing wire and persistence tests**

Add to `domain_contract.rs`:

```rust
#[test]
fn windows_platform_has_a_stable_wire_value() {
    assert_eq!(serde_json::to_value(Platform::Windows).unwrap(), serde_json::json!("windows"));
    assert_eq!(
        serde_json::from_value::<Platform>(serde_json::json!("windows")).unwrap(),
        Platform::Windows
    );
}
```

Add a Windows application repository round-trip in `sqlite_repository.rs`; assert `Platform::Windows` and the literal `aumid:Contoso.Chat_abc!App` survive storage.

- [ ] **Step 2: Observe RED**

Run:

```powershell
cargo test -p autologin-core --test domain_contract windows_platform_has_a_stable_wire_value
cargo test -p autologin-core --test sqlite_repository
```

Expected: compile failure because `Platform::Windows` is absent.

- [ ] **Step 3: Add the minimal variant and exhaustive scan branch**

```rust
pub enum Platform {
    Macos,
    Windows,
}
```

Update `scan.rs`:

```rust
let rank = match platform {
    Platform::Macos => 0,
    Platform::Windows => 1,
};
```

- [ ] **Step 4: Verify GREEN and regressions**

Run `cargo test -p autologin-core` and `cargo test -p autologin-desktop scan`. Expected: pass without warnings.

- [ ] **Step 5: Commit**

```powershell
git add crates/autologin-core app/src-tauri/src/scan.rs
git commit -m "feat: add Windows platform identity"
```

### Task 2: Create the Windows crate and typed launch targets

**Files:**
- Create: `crates/platform-windows/Cargo.toml`
- Create: `crates/platform-windows/src/lib.rs`
- Create: `crates/platform-windows/src/error.rs`
- Create: `crates/platform-windows/src/target.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Produces `WindowsLaunchTarget::{Executable(PathBuf), Aumid(String)}`.
- Produces `parse(&str) -> Result<Self, AppError>` and `encode(&self) -> String`.
- Produces `record_last_status(u32)` and `credential_last_status() -> Option<i32>`.

- [ ] **Step 1: Add the crate shell and exact dependencies**

Use `windows = 0.61.3` with `ApplicationModel`, `Foundation`, `Foundation_Collections`, `Management_Deployment`, `Storage`, `Win32_Foundation`, `Win32_Graphics_Gdi`, `Win32_Security_Credentials`, `Win32_Storage_FileSystem`, `Win32_System_Com`, `Win32_System_DataExchange`, `Win32_System_Memory`, `Win32_System_Registry`, `Win32_System_Threading`, `Win32_UI_Shell`, and `Win32_UI_WindowsAndMessaging`. Add shared `async-trait`, `autologin-core`, `base64`, `secrecy`, `tokio`, `uuid`, and `zeroize` dependencies. Export only `error` and `target` initially; do not create placeholder platform implementations.

- [ ] **Step 2: Write failing typed-target tests**

```rust
#[test]
fn target_round_trips_without_guessing() {
    let exe = WindowsLaunchTarget::Executable(PathBuf::from(r"C:\Apps\Chat\chat.exe"));
    let aumid = WindowsLaunchTarget::Aumid("Contoso.Chat_abc!App".into());
    assert_eq!(WindowsLaunchTarget::parse(&exe.encode()).unwrap(), exe);
    assert_eq!(WindowsLaunchTarget::parse(&aumid.encode()).unwrap(), aumid);
}

#[test]
fn target_rejects_relative_or_untyped_commands() {
    for value in ["exe:chat.exe", "aumid:", "powershell:calc"] {
        assert_eq!(WindowsLaunchTarget::parse(value).unwrap_err().code(), "validation.invalid_field");
    }
}
```

- [ ] **Step 3: Observe RED**

Run `cargo test -p platform-windows target::tests`. Expected: missing type compile failure.

- [ ] **Step 4: Implement strict target parsing**

Use only `exe:<absolute .exe path>` and `aumid:<nonblank AUMID>`. Reject NUL, relative paths, unknown schemes, non-`.exe` paths, and strings over 4096 characters.

- [ ] **Step 5: Test and implement stable error mapping**

Test literal mappings before code:

```rust
assert_eq!(credential_error(ERROR_NOT_FOUND.0).code(), "credential.not_found");
assert_eq!(credential_error(ERROR_NO_SUCH_LOGON_SESSION.0).code(), "credential.unavailable");
assert_eq!(credential_error(ERROR_ACCESS_DENIED.0).code(), "credential.denied");
```

Store the last native status in an `AtomicI32`; never forward arbitrary system messages in error params.

- [ ] **Step 6: Verify and commit**

Run `cargo fmt --all -- --check` and `cargo test -p platform-windows`. Commit:

```powershell
git add Cargo.toml Cargo.lock crates/platform-windows
git commit -m "feat: scaffold native Windows platform crate"
```

### Task 3: Implement Credential Manager storage

**Files:**
- Create: `crates/platform-windows/src/credentials.rs`
- Modify: `crates/platform-windows/src/lib.rs`
- Create: `crates/platform-windows/tests/credentials_native.rs`

**Interfaces:**
- Produces `WindowsCredentialStore::new(service: String) -> Self`.
- Implements `CredentialStore::{put,reveal,delete}`.

- [ ] **Step 1: Write a real failing lifecycle test**

```rust
#[tokio::test]
async fn generic_credential_round_trip_and_delete_are_real() {
    let store = WindowsCredentialStore::new(format!("LoginDeck.Tests.{}", uuid::Uuid::new_v4()));
    let reference = store.new_reference().unwrap();
    store.put(&reference, "fixture", SecretString::from("correct horse")).await.unwrap();
    assert_eq!(store.reveal(&reference, "test").await.unwrap().expose_secret(), "correct horse");
    store.delete(&reference).await.unwrap();
    assert_eq!(store.reveal(&reference, "missing").await.unwrap_err().code(), "credential.not_found");
}
```

The integration test owns a unique namespace and uses a test-only drop guard for panic cleanup.

- [ ] **Step 2: Observe RED**

Run `cargo test -p platform-windows --test credentials_native -- --nocapture`. Expected: missing store compile failure.

- [ ] **Step 3: Implement minimal native operations**

Use `CRED_TYPE_GENERIC`, `CRED_PERSIST_LOCAL_MACHINE`, and `LoginDeck/<service>/<uuid>` targets. Before `CredWriteW`, call `CredReadW`; existing data returns `storage.conflict`. Free `CredReadW` results with an RAII `CredFree` guard. Hold the secret in a mutable UTF-8 `Vec<u8>`, reject values above `CRED_MAX_CREDENTIAL_BLOB_SIZE`, and `zeroize()` on every path.

- [ ] **Step 4: Add failing no-overwrite and size tests**

One test writes `first`, attempts `second`, expects `storage.conflict`, then reveals `first`. Another uses `CRED_MAX_CREDENTIAL_BLOB_SIZE + 1` ASCII bytes, expects `validation.invalid_field` for `password`, then proves no item was created.

- [ ] **Step 5: Verify GREEN**

Run:

```powershell
cargo test -p platform-windows --test credentials_native -- --test-threads=1
cargo test -p autologin-core --test vault_contract
```

Expected: pass; output contains no password.

- [ ] **Step 6: Commit**

```powershell
git add crates/platform-windows/src/credentials.rs crates/platform-windows/src/lib.rs crates/platform-windows/tests/credentials_native.rs
git commit -m "feat: store Windows secrets in Credential Manager"
```

### Task 4: Implement guarded Windows clipboard cleanup

**Files:**
- Create: `crates/platform-windows/src/clipboard.rs`
- Modify: `crates/platform-windows/src/lib.rs`
- Create: `crates/platform-windows/tests/clipboard_native.rs`

**Interfaces:**
- Produces `WindowsClipboard::new()` and `WindowsClipboardChangeToken(u32)`.
- Implements `Clipboard::{write,clear_if_unchanged}`.

- [ ] **Step 1: Write a serial failing native test**

```rust
#[tokio::test]
async fn cleanup_clears_only_its_own_clipboard_write() {
    let clipboard = WindowsClipboard::new();
    let first = clipboard.write(SecretString::from("LoginDeck fixture")).await.unwrap();
    clipboard.clear_if_unchanged(&first).await.unwrap();
    assert!(!unicode_text_is_available());

    let second = clipboard.write(SecretString::from("LoginDeck old")).await.unwrap();
    write_external_fixture("user replacement");
    clipboard.clear_if_unchanged(&second).await.unwrap();
    assert_eq!(read_external_fixture(), "user replacement");
}
```

Test-only helpers may read/restore clipboard state; the production class must not expose reads.

- [ ] **Step 2: Observe RED**

Run `cargo test -p platform-windows --test clipboard_native -- --test-threads=1`. Expected: missing clipboard type.

- [ ] **Step 3: Implement Unicode writes with RAII**

Use bounded retry around `OpenClipboard`, then `EmptyClipboard`, `GlobalAlloc(GMEM_MOVEABLE)`, `GlobalLock`, UTF-16 including NUL, `GlobalUnlock`, and `SetClipboardData(CF_UNICODETEXT)`. Transfer allocation ownership only after `SetClipboardData` succeeds. Return `GetClipboardSequenceNumber()` after the write.

- [ ] **Step 4: Implement race-safe conditional clear**

Compare the sequence number before and immediately after acquiring the clipboard. Call `EmptyClipboard` only if both equal the token. Exhausted retries map to `clipboard.unavailable`.

- [ ] **Step 5: Verify and commit**

Run `cargo test -p platform-windows --test clipboard_native -- --test-threads=1` and `cargo test -p autologin-core clipboard`. Commit:

```powershell
git add crates/platform-windows/src/clipboard.rs crates/platform-windows/src/lib.rs crates/platform-windows/tests/clipboard_native.rs
git commit -m "feat: add guarded Windows clipboard backend"
```

### Task 5: Build the launchable Win32 inventory

**Files:**
- Create: `crates/platform-windows/src/app_catalog.rs`
- Create: `crates/platform-windows/src/app_catalog/win32.rs`
- Create: `crates/platform-windows/src/app_catalog/shortcut.rs`
- Create: `crates/platform-windows/tests/fixtures/uninstall.json`
- Modify: `crates/platform-windows/src/lib.rs`

**Interfaces:**
- Produces internal `WindowsAppCandidate` with identity, name, version, typed target, icon source, signature identity, and source.
- Produces `normalize_uninstall_entry(entry, shortcuts) -> Option<WindowsAppCandidate>`.
- Produces `Win32Inventory::enumerate() -> Result<Vec<WindowsAppCandidate>, AppError>`.

- [ ] **Step 1: Create independent literal fixtures**

Include quoted `DisplayIcon` with icon index, `SystemComponent=1`, `ReleaseType=Update`, `ParentKeyName`, blank name, uninstall-only executable, `InstallLocation` plus matching shortcut, and duplicate HKCU/HKLM entries resolving to one executable. Expected targets are literal strings, not generated with production helpers.

- [ ] **Step 2: Write failing table-driven tests**

```rust
#[test]
fn inventory_keeps_only_launchable_user_apps() {
    for case in fixture_cases() {
        let actual = normalize_uninstall_entry(case.entry, &case.shortcuts);
        assert_eq!(actual.map(|x| x.target.encode()), case.expected_target, "{}", case.name);
    }
}
```

Add a separate test proving duplicate executable paths collapse deterministically.

- [ ] **Step 3: Observe RED**

Run `cargo test -p platform-windows app_catalog::win32::tests`. Expected: missing candidate/parser types.

- [ ] **Step 4: Implement pure normalization**

Implement and test these focused functions:

```rust
fn parse_display_icon(raw: &str) -> Option<(PathBuf, i32)>;
fn is_visible_product(entry: &UninstallEntry) -> bool;
fn choose_executable(entry: &UninstallEntry, shortcuts: &[Shortcut]) -> Option<PathBuf>;
fn executable_identity(path: &Path) -> Result<String, AppError>;
fn deduplicate_win32(items: Vec<WindowsAppCandidate>) -> Vec<WindowsAppCandidate>;
```

Reject known uninstall/update helper basenames unless a Start Menu shortcut explicitly designates one as user-facing.

- [ ] **Step 5: Test and implement all registry views**

Use a private `RegistrySource` boundary with a test source. Assert resulting inventory while ensuring these exact scopes are consumed once: HKCU 64-bit, HKCU 32-bit, HKLM 64-bit, HKLM 32-bit. Then implement `RegOpenKeyExW`, `RegEnumKeyExW`, and `RegQueryValueExW` with `KEY_WOW64_64KEY` / `KEY_WOW64_32KEY`, bounded values, and RAII key closure.

- [ ] **Step 6: Test and implement shortcut resolution**

Enumerate current-user and common Start Menu program folders. Resolve `.lnk` using `IShellLinkW` and `IPersistFile`; reject relative, network, missing, non-`.exe`, and reparse-point targets.

- [ ] **Step 7: Verify and commit**

Run `cargo test -p platform-windows app_catalog::win32::tests` and `cargo test -p platform-windows app_catalog::shortcut::tests`. Commit:

```powershell
git add crates/platform-windows/src/app_catalog.rs crates/platform-windows/src/app_catalog crates/platform-windows/tests/fixtures/uninstall.json crates/platform-windows/src/lib.rs
git commit -m "feat: discover launchable Win32 applications"
```

### Task 6: Add packaged applications and merge inventories

**Files:**
- Create: `crates/platform-windows/src/app_catalog/package.rs`
- Create: `crates/platform-windows/tests/fixtures/packages.json`
- Create: `crates/platform-windows/tests/catalog_native.rs`
- Modify: `crates/platform-windows/src/app_catalog.rs`

**Interfaces:**
- Produces `PackagedAppInventory::enumerate_current_user()`.
- Produces `merge_candidates(win32, packaged)`.
- Produces `WindowsApplicationCatalog::new()`.
- Implements `ApplicationCatalog::discover` and bounded `import_bundle`.

- [ ] **Step 1: Create packaged-app fixtures and failing parser tests**

Cover a normal app, two Applications in one package, framework, resource package, no Application, unavailable package, and packaged desktop app sharing an executable with a Win32 record. Assert literal output:

```rust
assert_eq!(
    parsed.iter().map(|x| x.target.encode()).collect::<Vec<_>>(),
    vec!["aumid:Contoso.Chat_abc!App", "aumid:Contoso.Tools_abc!Editor"]
);
```

- [ ] **Step 2: Observe RED**

Run `cargo test -p platform-windows app_catalog::package::tests`. Expected: missing packaged inventory.

- [ ] **Step 3: Implement current-user package enumeration**

Initialize WinRT, call `PackageManager::FindPackagesForUser("")`, skip framework/resource/unavailable packages, and parse each manifest Application declaration. Build AUMIDs from package family name plus application ID. Display-name or logo resolution failure may degrade fields but must not discard a valid Application.

- [ ] **Step 4: Test and implement merge behavior**

Failing tests must prove: packaged desktop duplicates prefer AUMID; duplicate AUMID appears once; equal display names do not merge; ordering is case-insensitive name then stable ID. Implement only those rules.

- [ ] **Step 5: Implement `discover`**

Run blocking native enumeration with `spawn_blocking`, cap raw/final counts at 4096, skip malformed records, and return `application.discovery_unavailable` only if both sources fail or the task fails. Convert results to `DiscoveredApplication` with `Platform::Windows`, `path_access_ref: None`, and `DiscoverySource::Automatic`.

- [ ] **Step 6: Test and implement bounded manual import**

Tests accept an absolute existing `.exe` and a directory with an unambiguous entry inside fixed depth/count limits. Tests reject `.bat`, `.cmd`, `.ps1`, relative paths, reparse points, and budget overflow. Implement `DiscoverySource::ManualImport`; never browse protected `WindowsApps` for manual import.

- [ ] **Step 7: Add a non-destructive live smoke test**

Call `discover`; assert every item is Windows, every target parses, IDs are unique, and count is at most 4096. Do not require a machine-specific app.

- [ ] **Step 8: Verify and commit**

Run `cargo test -p platform-windows app_catalog` and the live catalog test. Commit:

```powershell
git add crates/platform-windows/src/app_catalog.rs crates/platform-windows/src/app_catalog/package.rs crates/platform-windows/tests
git commit -m "feat: merge launchable Windows application inventories"
```

### Task 7: Add icons and verified launch

**Files:**
- Create: `crates/platform-windows/src/icons.rs`
- Create: `crates/platform-windows/src/launch.rs`
- Modify: `crates/platform-windows/src/app_catalog.rs`
- Modify: `crates/platform-windows/src/lib.rs`
- Modify: `crates/platform-windows/tests/catalog_native.rs`

**Interfaces:**
- Produces `application_icon(path: &Path) -> Option<String>` for existing callers.
- Produces internal target-aware icon lookup.
- Completes `ApplicationCatalog::verify_and_launch`.

- [ ] **Step 1: Write failing icon behavior tests**

Use a controlled fixture executable/resource, not a user-specific hard-coded path. Assert valid output starts with `data:image/png;base64,iVBORw0KGgo`; missing/malformed targets return `None` without failing discovery.

- [ ] **Step 2: Implement icon extraction**

Use Shell APIs for Win32 icons and manifest logo assets for packaged apps. Render to 32-bit BGRA, encode PNG, then base64. Wrap every HICON/HBITMAP/HDC resource in RAII.

- [ ] **Step 3: Write failing launch-verification tests**

Use the test binary in a child mode. Prove unchanged Win32 identity launches with nonzero PID; missing target returns `application.not_found`; changed identity returns `application.signature_changed`; raw command lines are rejected. Test AUMID revalidation separately from a live Store-app activation check.

- [ ] **Step 4: Implement Win32 verified launch**

Reparse the typed target, canonicalize it, reject reparse points, recompute identity, compare with the persisted signature, then call `CreateProcessW` with an explicit application path and no shell interpretation. Close native handles after capturing PID.

- [ ] **Step 5: Implement AUMID activation**

Re-enumerate current-user package applications, verify AUMID and signature, then use out-of-process `IApplicationActivationManager::ActivateApplication`; return its PID.

- [ ] **Step 6: Verify and commit**

Run icon, launch, and serial native catalog tests. Commit:

```powershell
git add crates/platform-windows/src/icons.rs crates/platform-windows/src/launch.rs crates/platform-windows/src/app_catalog.rs crates/platform-windows/src/lib.rs crates/platform-windows/tests/catalog_native.rs
git commit -m "feat: verify and launch Windows applications"
```

### Task 8: Select Windows in the runtime and build Tauri

**Files:**
- Modify: `crates/platform-runtime/Cargo.toml`
- Modify: `crates/platform-runtime/src/lib.rs`
- Modify: `app/src-tauri/src/commands/applications.rs`
- Modify: `app/src-tauri/tests/command_contract.rs`
- Modify: `app/src-tauri/tauri.conf.json`
- Create: `app/src-tauri/icons/icon.ico`

**Interfaces:**
- Produces Windows native aliases, `CREDENTIAL_BACKEND = "windows_credential_manager"`, and `PLATFORM = Platform::Windows`.
- Preserves all current command names and response schemas.

- [ ] **Step 1: Write a failing target-gated facade test**

Construct all three Windows aliases and assert `PLATFORM == Platform::Windows` and the backend string. Run `cargo test -p platform-runtime`; observe the current non-macOS compile error.

- [ ] **Step 2: Implement target selection**

Add the Windows target dependency and separate `selected` modules for macOS/Windows. Keep compile failure for other OS. Export Windows catalog, clipboard, credential store, icon facade, status, and constants. Do not export a fake Windows `login` module.

- [ ] **Step 3: Protect the bounded command surface**

Confirm `platform_runtime::login` matches occur only in uncompiled historical modules. Add a command-contract test proving no fill/switch command is registered. If a compiled module imports login APIs, gate/remove that dead registration instead of inventing Windows automatic login.

- [ ] **Step 4: Make icon lookup target-aware**

Write a command-level test with a stored AUMID record first. Change the facade/call site so icons resolve from the trusted stored record and typed target, rather than treating every launch target as `Path`.

- [ ] **Step 5: Add the Windows bundle icon**

Generate `icon.ico` from the canonical `icon.png` using the Tauri icon command, inspect standard Windows sizes, and list `icons/icon.ico` in `tauri.conf.json`. The real Tauri build verifies this generated asset.

- [ ] **Step 6: Build and smoke-test**

Run:

```powershell
cargo test -p platform-runtime
cargo test -p autologin-desktop
pnpm --dir app typecheck
pnpm --dir app build
pnpm --dir app tauri build --debug
```

Then verify UI load, password save/copy, replacement clipboard preservation, mixed Win32/MSIX scan, one Win32 launch, one packaged launch, and no active automatic-login controls.

- [ ] **Step 7: Commit**

```powershell
git add crates/platform-runtime app/src-tauri
git commit -m "feat: build LoginDeck with the Windows runtime"
```

### Task 9: Add verification, evidence, and CI

**Files:**
- Create: `scripts/verify-windows.ps1`
- Create: `docs/testing/2026-09-18-windows-foundation.md`
- Create after local success: `.github/workflows/windows.yml`
- Modify: `README.md`
- Modify: `docs/repository-architecture.md`
- Modify: `crates/platform-windows/README.md`

**Interfaces:**
- Produces canonical command `powershell -ExecutionPolicy Bypass -File scripts/verify-windows.ps1`.

- [ ] **Step 1: Create the executable verification script**

Set `$ErrorActionPreference = 'Stop'`, resolve root from `$PSScriptRoot`, reject non-Windows/non-x64 hosts, then run:

```powershell
cargo fmt --all -- --check
cargo test --workspace --all-targets
pnpm --dir app install --frozen-lockfile
pnpm --dir app typecheck
pnpm --dir app test -- --run
pnpm --dir app build
pnpm --dir app tauri build --debug
```

- [ ] **Step 2: Run locally and fix through regression tests**

Run `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-windows.ps1`. Expected: exit 0. Any code defect first receives a focused failing test.

- [ ] **Step 3: Record evidence**

Document Windows edition/build, architecture, Rust/Node/pnpm versions, exact command, exit status, test counts, artifact path, and Task 8 manual checks. Mark AUMID activation unverified unless actually exercised.

- [ ] **Step 4: Update documentation**

Add Windows prerequisites and command to README. Replace the placeholder Windows crate README with implemented scope and automatic-login exclusion. Update architecture status without claiming full macOS parity.

- [ ] **Step 5: Add CI only after local success**

Use `windows-2022`, Node 22, pnpm 11.19.0, Rust 1.89.0, and caches. Run the script. If the hosted runner lacks an interactive clipboard/session, explicitly separate those local-only tests; never label skipped native checks as passing.

- [ ] **Step 6: Final verification**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-windows.ps1
git diff --check
git status --short
```

Expected: verification succeeds, diff check is clean, and only intended Windows foundation changes remain.

- [ ] **Step 7: Commit**

```powershell
git add scripts/verify-windows.ps1 .github/workflows/windows.yml README.md docs/repository-architecture.md crates/platform-windows/README.md docs/testing/2026-09-18-windows-foundation.md
git commit -m "ci: verify the Windows foundation build"
```

## Final Verification Checklist

- [ ] Windows platform wire value and SQLite round-trip pass without changing macOS data.
- [ ] Credential tests prove create/read/delete, no overwrite, size rejection, and cleanup.
- [ ] Clipboard tests prove unchanged data clears and replacement data survives.
- [ ] Win32 inventory covers four registry scopes and returns only valid executable targets.
- [ ] Packaged inventory returns only current-user launchable Application entries.
- [ ] Merge prefers AUMID for packaged desktop duplicates and never merges only by name.
- [ ] Win32 and AUMID launch both revalidate identity and return a PID.
- [ ] Icons yield PNG data URLs or safely return `None`.
- [ ] Shared frontend and command schemas remain unchanged.
- [ ] No Windows automatic-login implementation is introduced.
- [ ] Full Windows verification succeeds on a real supported x64 machine.
- [ ] Evidence distinguishes automated, manual, skipped, and unverified checks.
- [ ] Windows CI exists only after local verification passes.

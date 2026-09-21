# Windows foundation verification

Evidence date: 2026-09-20, Asia/Hong_Kong (+08:00). This records the implemented Windows foundation,
not release approval or full native UI acceptance. The approved design is
[Windows foundation](../superpowers/specs/2026-09-18-windows-foundation-design.md).

> Historical milestone record: repository layout, Edge integration, hosted CI and release status
> changed after this evidence was captured. Use the root README and current GitHub Actions runs for
> present-day release instructions and status; the observations below remain the dated acceptance
> evidence for the Windows foundation milestone.

## Host and tools

| Item | Observed value |
| --- | --- |
| OS | Microsoft Windows 11 Pro, 25H2, build 26200.9457, x64 |
| PowerShell | Windows PowerShell 5.1.26100.9444; OS and process architecture both X64 |
| Rust / Cargo | rustc 1.89.0 (29483883e, 2025-08-04); cargo 1.89.0 (c24e10642, 2025-06-23) |
| Target | `x86_64-pc-windows-msvc`; repository toolchain 1.89.0 with rustfmt |
| Node / Corepack / pnpm | Node v24.19.0; Corepack 0.35.0; explicit pnpm 11.19.0 |
| Native build tools | Visual Studio Build Tools 2022 17.14.41 (17.14.37710.0), MSVC 14.44.35207, Windows SDK 10.0.26100.0 |
| WebView2 | Installed application directories include 153.0.4234.32, .46 and .48; this is installed-file evidence, not identification of the loaded runtime |
| Tauri / frontend | tauri-cli 2.11.4; Rust Tauri 2.11.5; Vite 7.3.6; Vitest 4.1.11 |
| Git | 2.55.0.windows.3; `core.autocrlf=true`; all nine migration SQL files have `i/lf w/lf attr/text eol=lf` |

`Get-ComputerInfo` reports the Windows 11 OS name and build above; the legacy registry ProductName
still says Windows 10 Pro, so it was not used to identify the edition. The supported desktop targets
are Windows 10 22H2 and Windows 11 x64; only this Windows 11 machine was exercised. CI pins Node 22;
the local successful run used Node 24.19.0 and does not prove the Node 22 hosted combination.

## Canonical verification

Product root on this host:
`<workspace>\LoginDeck`.
The outer Git root contains that nested `LoginDeck` directory.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-windows.ps1
```

The script rejects non-Windows/non-x64 hosts and non-x64 PowerShell processes, resolves its own
product root, and restores the caller directory. Every native failure stops the sequence and
preserves its exit code. PowerShell errors return nonzero. Its actual sequence is:

1. `cargo fmt --all -- --check`
2. `corepack pnpm@11.19.0 --dir app install --frozen-lockfile`
3. `node scripts/prepare-edge-bundle.mjs` (ignored resources required by fresh-checkout desktop tests)
4. `cargo test --locked --workspace --all-targets -- --test-threads=1`
5. `node --test scripts/bundle-native-host.test.mjs scripts/tauri.test.mjs scripts/verify-windows.test.mjs`
6. `corepack pnpm@11.19.0 --dir app typecheck`
7. `corepack pnpm@11.19.0 --dir app test -- --run`
8. `corepack pnpm@11.19.0 --dir app build`
9. `corepack pnpm@11.19.0 --dir app tauri build --debug`

The package-manager argument is read from `app/package.json`'s exact pin. This host's outer Corepack
default is pnpm 12.4.2; a bare root-cwd `pnpm --dir app` previously failed `ERR_PNPM_BAD_PM_VERSION`.
Strict version checks remain enabled. No `--ignore-scripts`, native-test filter, bundle exclusion
or package pin change was added.

The first complete canonical run finished at **17:36:10 +08:00 with exit 0**, before Windows CI was
created. The Task 9 post-change run finished at **17:51:48 +08:00 with exit 0**; both had 236 Rust
tests passed. After the final-review test portability correction described below, the latest full
canonical run finished at **23:11:19 +08:00 with exit 0** and the counts below. Only documentation
and the task report were finalized afterward. Logs are in the outer Git root's ignored
`.superpowers/sdd/2026-09-18-windows-foundation/`: `task-9-local-before-ci.log`,
`task-9-final-verification.log` and `final-fix-verification.log`. These local coordination files
are not shipped with the repository.

| Check | Local result |
| --- | --- |
| Formatting | Exit 0 |
| Full Rust workspace/all targets, serial | 238 harness-managed passed, 0 failed, 2 intentional ignores; separate controlled Win32 launch harness passed |
| Windows crate within workspace | 65 unit + 3 catalog + 10 clipboard + 7 credential + 1 test-boundary audit passed, plus controlled launch harness |
| Core / desktop / runtime | 98 core; 19 desktop (15 unit + 4 contract); 3 facade tests passed |
| Existing macOS crate on Windows | 29 non-native/fixture tests passed; this is not native macOS verification |
| Script regressions | 8 passed: 4 resource filename, 1 Tauri wrapper, 3 Windows verifier |
| Frontend | Typecheck passed; 13 files / 64 tests passed; production build transformed 180 modules |
| Full Tauri debug build | EXE plus MSI and NSIS generated successfully; release native-host resource preparation included |

The two Rust ignores are `mutation_lock::tests::child_lock_probe` (explicitly invoked by its parent
lock test) and `controlled_package_activation_opt_in` (no controlled installed package supplied).
The latter is a real acceptance gap. Platform-gated macOS native tests are not executable here.
Existing off-platform macOS dead-code warnings remain (8 library, 2 test warnings); Rollup reports
two unrecognized PURE annotations in Zod and removes the comments. These warnings do not change
the successful exit status and were not suppressed.

## Regression and CI evidence

Final review found that the unconditional Windows workspace crate selected tests using Windows
absolute-path and parent/file-name behavior on macOS. Twelve such test cases now have Windows cfg
gates; the AUMID round-trip remains portable as a separate test. No production validation changed.
The new `platform-windows/tests/test_boundaries.rs` source audit failed first with the twelve
ungated selections, then passed after the correction. It parses crate/module/function cfg gates
and every integration-test source, requiring the macOS selection to match an explicitly reviewed
portable set. It also checks that the custom non-Windows launch harness has an empty entry point.
The audit selects 26 portable tests on macOS and 87 tests on Windows (including the package ignore).
It runs during the canonical workspace test stage, so new ungated tests require portability review.

The macOS preflight uses `cargo test --workspace --locked`. Rust 1.89.0 `--print cfg` for both
`x86_64-apple-darwin` and `aarch64-apple-darwin` confirms `unix`, `target_os="macos"` and no Windows
predicate, matching the audit. Only the Windows target standard library is installed here. This
proves source test selection for the present cfg structure; no native macOS build or execution is
claimed. Changes to test bodies still require portability review, and macro-generated tests would
need explicit audit support. RED/GREEN logs and the full surface audit are retained as
`final-fix-red.log`, `final-fix-green.log` and `final-fix-report.md` in the ignored SDD directory.

The verifier tests were written first: all 3 failed with ENOENT for the absent script, then all 3
passed after implementation. They execute the real script from an unrelated cwd, copy it under a
path with spaces, an ampersand and an apostrophe, and substitute real tiny Node child processes for
expensive native build commands. They verify the entire command sequence, exact pinned invocation,
resource preparation before workspace tests, successful stderr diagnostics, every one of the nine
stage failures stopping immediately with exit 37, and caller-directory restoration. The real host
guard is separately executed with Windows x64, Unix, x86 and ARM64/x64-emulation inputs; unsupported
cases reject without adding a production host-bypass option.

The deferred Task 1 repository regression `application_update_round_trips_windows_platform_and_aumid`
inserts a macOS row, updates that same row to Windows/AUMID, and asserts the reloaded full record.
It passed on existing production code; no production correction or new RED claim was needed.

CI is located at **the outer Git root** `.github/workflows/windows.yml`, with `LoginDeck` working
directory and cache/lock paths. The plan's apparent product-root location would be nested and
undiscoverable by GitHub in this checkout. The existing nested macOS workflow was not relocated.
Windows CI uses `windows-2022`, Node 22, Corepack 0.35.0, pnpm 11.19.0, Rust 1.89.0/rustfmt,
pnpm-store and Rust caches, and the canonical command. No native test is silently skipped for CI;
an unavailable credential/clipboard session must fail visibly. No hosted run has been dispatched
or observed, and no remote is configured in this checkout.

The workflow parses with js-yaml 4.1.0, and its PowerShell run blocks parse locally. A controlled
execution of the actual package-manager preparation block reproduced Windows PowerShell 5.1's
UTF-16 `>>` output: strict UTF-8 decoding failed on byte FF. Explicit `Out-File -Encoding utf8`
fixed it, and the same check then preserved the exact cache path. This validates local syntax and
output behavior; it is not a claim of a successful GitHub Actions run.

## Task 8 startup failure and fix

Task 8 initially reproduced immediate standalone EXE exit during setup with sanitized
`storage.database`. Read-only inspection showed all nine migrations in the existing user database
had canonical LF SHA-384 checksums, while Git's CRLF checkout conversion made the Windows binary
embed different SQLx checksums. SQLx correctly rejected `VersionMismatch(1)`.

The regression `sqlite::tests::startup_reopens_database_created_from_canonical_lf_migrations`
first failed against a separate LF-initialized fixture database; `.gitattributes` then pinned
`/migrations/*.sql text eol=lf`. The regression passed with its saved setting intact. Strict SQLx
validation and the user's database/history were preserved. A database first created by the defective
CRLF binary would still be rejected; no silent checksum rewrite or general recovery policy was added.

Retained Task 8 live evidence: its fixed EXE started at **17:18:21**, PID **109224**, against the
existing application database. At **17:19:13** it still had title **LoginDeck**, HWND **7733536** and
`Responding=True`, with empty stderr. This is the earlier fixed artifact's process/window evidence,
not a visual interaction check of the final Task 9 rebuild. The original log is
`.superpowers/sdd/2026-09-18-windows-foundation/task-8-fix1-startup-window.log` in the outer Git root.
The task's UI surface exposed no usable native-app controls. Window contents/layout were not observed.

## Automated / manual / skipped / unverified matrix

| Behavior | Evidence and status |
| --- | --- |
| Credential create/read/delete, no overwrite, size limits, cleanup | Automated real Credential Manager tests passed with owned unique fixtures |
| Account update/recovery services | Automated shared service/SQLite regressions passed; live password UI unverified |
| Own clipboard clear, independent replacement preservation, cancellation | Automated native tests passed; shared scheduler tests passed; live copy button and 30-second UI observation unverified |
| Mixed Win32/current-user package discovery, bounded import and dedup | Automated fixtures and live native source/catalog tests passed; visible scan UI unverified |
| Win32 verified launch and PID/argv | Automated controlled child harness passed; no manual user-application launch claimed |
| AUMID identity checking | Automated callback/record validation passed; real controlled MSIX activation skipped/unverified |
| Icons and package logo stream handling | Automated Win32 resource/PNG and package stream/dispatch tests passed; installed-package logo selection unverified |
| Standalone app startup against existing DB | Task 8 live responsive titled HWND evidence above; no visible interaction/layout acceptance |
| Current no-login command/UI boundary | Automated command/frontend contracts pass; no Windows automatic login or UI Automation implemented |
| Generated ICO and MSI/NSIS | Build and file/container inspection passed; installers never executed |
| Windows 10 22H2 / ARM / macOS | Windows 10 and native macOS unverified; ARM and x86 unsupported |
| Hosted Windows CI / Node 22 combination | Configuration locally validated; remote execution unverified |
| Windows Edge native messaging install/capture | Outside this milestone; shared resources in the bundle do not enable it |

No installer was run, published or signed. No UI password/copy interaction, controlled package
activation, Windows 10 test or native macOS test is claimed.

## Artifacts

Product root used for the run: `<workspace>\LoginDeck`.
The following paths are relative to that root and identify local debug outputs, not installed apps.
Hashes below are SHA-256 and are refreshed after the final canonical build.

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `app/src-tauri/icons/icon.ico` | 21613 | `94AE2A8D513E248DC8F1D76DA8E0588A837E9C8F65358E767155D0588A08449A` |
| `target/debug/autologin-desktop.exe` | 21988864 | `DD3F7A15A4AD3880920A833EA98599F6B94048A0F3EE52374FD81FA2C101398D` |
| `target/debug/bundle/msi/LoginDeck_0.1.0_x64_en-US.msi` | 7958528 | `F63ACFEBDC21496E62D4ACE7AA7CC7E441F8BABDB14F2AC4E9543D521E48B4E9` |
| `target/debug/bundle/nsis/LoginDeck_0.1.0_x64-setup.exe` | 5012997 | `764EFAC94FCAC754E0E565EB087A098A2933FE5804C8DECB23DE5E165F5AD103` |

The generated ICO retains the canonical Task 8 asset and contains six PNG-backed entries at
32, 16, 24, 48, 64 and 256 pixels, with valid directory ranges. Frontend output is `app/dist/index.html`
and its hashed assets. Bundled native host is `app/src-tauri/resources/edge/autologin-native-host.exe`;
the Tauri target-specific build source is `target/x86_64-pc-windows-msvc/release/autologin-native-host.exe`.

Win32 `signature_identity` is a metadata fingerprint (canonical path, volume/file ID, size, last-write
time), while package identity binds registration full name and AUMID. Neither proves file content,
Authenticode or publisher trust. File handles narrow replacement races, but verification and native
process creation are not an OS transaction. Credential Manager's in-process exclusion does not
provide atomic creation against a competing external process. These limitations remain unchanged.

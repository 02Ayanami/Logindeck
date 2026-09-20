# Public release readiness — 2026-09-21

## Scope

This record covers the local preparation of LoginDeck for a public GitHub repository and unsigned
installer releases for Windows x64 and Apple Silicon macOS. It does not record a publication: no
Git remote, tag, GitHub repository, or GitHub Release was created.

## Passed locally

- The tracked project is rooted at the Git repository root; no tracked product files remain below
  the former `LoginDeck/` wrapper directory.
- `LICENSE`, `README.md`, `SECURITY.md`, and `CONTRIBUTING.md` pass the public-document contract.
- Windows verification uses `windows-2022`; macOS verification and release builds use `macos-14`
  and assert `arm64`. Verification jobs have read-only repository permission.
- The release workflow checks an exact `vMAJOR.MINOR.PATCH` tag against `app/package.json`, builds
  Windows NSIS and macOS DMG installers in separate native jobs, and gives write permission only to
  the final job after both artifacts are present.
- Release-helper tests pass for stable public filenames, installer-type filtering, ambiguous input
  rejection, version mismatch rejection, and SHA-256 generation.
- The Windows dry run collected exactly
  `LoginDeck-0.1.0-windows-x64-setup.exe`; its helper-produced SHA-256 matched PowerShell's
  independently calculated SHA-256.
- `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-windows.ps1` passed from the
  flattened root. This included Rust workspace tests, native Windows credential/clipboard/catalog/
  Edge checks, 14 frontend test files with 66 tests, type checking, frontend production build, and
  unsigned x64 MSI and NSIS debug bundles. Bundles were built but not installed.
- The final local contract run passed 15 tests with zero failures. `git diff --check` passed for the
  complete branch against the pre-migration base `fbdd6de`.

## Public-source audit

The audit checks current tracked files and every blob reachable from Git refs without printing any
matched secret value. No secret patterns, credential/signing/database files, generated installers,
or tracked files larger than 10 MiB were found. Current tracked files contain no non-fixture local
absolute paths.

Four historical path findings remain, all in two old test records and their former nested paths:

- `docs/testing/2026-09-18-windows-foundation.md`
- `docs/testing/2026-09-21-windows-edge-connection.md`
- the same two paths below the historical `LoginDeck/` wrapper

They disclose the former local Windows username/worktree or AppData location, but contain no secret,
token, password, or application data. The current versions use `<workspace>`, `%APPDATA%`, and
`%LOCALAPPDATA%`. Publishing the existing history therefore remains gated on an explicit choice:
accept these low-risk historical paths, or authorize a history rewrite before the first push.

## Remote-only verification gap

This Windows host cannot produce or execute an Apple Silicon DMG. The macOS ARM64 build must pass on
GitHub's `macos-14` runner before creating the first `v0.1.0` tag. The release workflow's manual mode
defaults to build-only, so it can validate both native artifacts without publishing a Release.

## Publication gate

Before publication, provide the destination GitHub repository and resolve the historical-path
choice above. Repository creation, adding a remote, pushing, tagging, and releasing require a final
explicit confirmation.

# GitHub Public Release Implementation Plan

**Goal:** Flatten LoginDeck into a standard public GitHub repository and add reproducible Windows x64 and macOS Apple Silicon CI plus unsigned tag-driven GitHub Releases.

**Design:** `docs/superpowers/specs/2026-09-21-github-public-release-design.md`

**Constraints:** Preserve Git history through moves, do not publish or create a remote, do not create a release tag, do not add signing/notarization, and stop publication if the source or history audit finds a real secret.

## Task 1: Flatten the Repository Safely

**Files:** all tracked paths under `LoginDeck/`, outer `.github/workflows/windows.yml`

- [ ] Record the current branch, HEAD, worktree status, tracked-file count, and wrapper/root path invariants.
- [ ] Add a path-contract test script that fails while `Cargo.toml`, `README.md`, and `app/` are not at the Git root or workflows are split across two `.github` directories.
- [ ] Move every tracked product path from `LoginDeck/` to the Git root using Git-aware moves; merge workflows into root `.github/workflows/` without overwriting either platform workflow.
- [ ] Move the product `.gitignore` and `.gitattributes` to the root and retain ignore rules for `.superpowers/`, `.worktrees/`, `target/`, prepared Edge resources, package stores, databases, and local coordination artifacts.
- [ ] Update path-dependent tests, workflow paths, documentation links, and scripts that referred to the wrapper directory.
- [ ] Verify tracked-file count parity, no residual tracked `LoginDeck/` prefix, no untracked product duplicates, and Git detects moves rather than delete/recreate where possible.
- [ ] Commit as `refactor: flatten the public repository layout`.

## Task 2: Add Public Project Metadata and Documentation

**Files:** `LICENSE`, `SECURITY.md`, `CONTRIBUTING.md`, `README.md`

- [ ] Add a documentation contract test asserting root metadata exists, contains no placeholders, and README states Windows x64, Apple Silicon-only macOS, unsigned learning builds, and manual Edge extension loading.
- [ ] Add the MIT License for `LoginDeck contributors`.
- [ ] Add GitHub Security Advisory reporting instructions and explicit warnings not to disclose credentials, databases, or passwords in public issues.
- [ ] Add contribution instructions with Rust 1.89.0, Node 22, pnpm 11.19.0, both canonical platform verification commands, formatting, tests, and scope boundaries.
- [ ] Revise README links and claims for the flattened repository and public unsigned releases; remove stale statements about unsupported Windows Edge integration or unexecuted installer validation.
- [ ] Run documentation/link/path contracts and scan for placeholders.
- [ ] Commit as `docs: prepare LoginDeck for public contribution`.

## Task 3: Normalize Windows and Apple Silicon CI

**Files:** `.github/workflows/windows.yml`, `.github/workflows/macos.yml`, workflow contract tests

- [ ] Add workflow source-contract tests proving both files live under root `.github/workflows`, use `contents: read`, target `main`, and invoke the canonical root scripts.
- [ ] Update Windows cache and working-directory paths for the flattened root while retaining `windows-2022` x64 and full native verification.
- [ ] Update macOS to use ARM64 `macos-14`, assert `uname -m` is `arm64`, use root lockfile paths, and run the canonical macOS verifier.
- [ ] Validate workflow YAML syntax and workflow contracts locally; run the Windows canonical verifier.
- [ ] Record that macOS runner success remains pending until the first GitHub Actions run.
- [ ] Commit as `ci: verify Windows and Apple Silicon macOS`.

## Task 4: Add Deterministic Release Packaging

**Files:** `.github/workflows/release.yml`, `scripts/check-release-version.mjs`, `scripts/collect-release-assets.*`, associated tests

- [ ] Write failing tests for tag/package version equality, supported `vMAJOR.MINOR.PATCH` parsing, exact Windows NSIS and macOS ARM64 DMG discovery, stable public filenames, and rejection of MSI/debug/raw app assets.
- [ ] Implement a platform-neutral version checker using `app/package.json` as the source of truth.
- [ ] Implement bounded release-asset collection that accepts exactly one expected native installer per platform and emits SHA-256 data without following arbitrary user paths.
- [ ] Add a tag/manual release workflow with separate native build jobs and a final publication job; only the final job receives `contents: write`.
- [ ] Require both platform artifacts before release creation and upload exactly the two installers plus `SHA256SUMS.txt`.
- [ ] Embed fixed release notes covering learning status, unsigned packages, Apple Silicon-only macOS, Windows x64, and in-app Edge installation guidance.
- [ ] Validate YAML, run all release helper tests, and exercise Windows asset collection against the built NSIS installer without publishing.
- [ ] Commit as `ci: publish unsigned cross-platform releases`.

## Task 5: Audit the Public Source and Git History

**Files:** `scripts/audit-public-repository.*`, audit tests, `docs/testing/2026-09-21-public-release-readiness.md`

- [ ] Write tests for detection of private-key headers, GitHub/cloud/API token shapes, tracked `.env`/database/credential/signing files, generated installers, local absolute paths, and oversized tracked binaries, including history-blob scanning.
- [ ] Implement a read-only audit script that checks HEAD and every reachable Git object without printing secret values.
- [ ] Add allowlisted documentation/test fixtures only when clearly synthetic; never suppress a real finding by broad path exclusion.
- [ ] Run the audit and classify every finding. If a real secret or personal artifact exists, stop before any remote creation and report the exact remediation boundary.
- [ ] Document current-file, history, large-file, workflow-permission, generated-artifact, and local-path results.
- [ ] Commit as `security: audit the repository for public release`.

## Task 6: Final Local Release Readiness Verification

**Files:** `docs/testing/2026-09-21-public-release-readiness.md`

- [ ] Run root layout, metadata, workflow, release-helper, and public-audit tests from a clean worktree.
- [ ] Run `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-windows.ps1` from the flattened root and confirm the unsigned NSIS build.
- [ ] Verify release asset collection produces only `LoginDeck-<version>-windows-x64-setup.exe` plus its checksum contribution.
- [ ] Inspect the complete branch diff against the pre-migration base for accidental generated files, private data, stale wrapper paths, excessive workflow permissions, and unsupported claims.
- [ ] Confirm no remote exists and no tag/release/publication action occurred.
- [ ] Update the readiness record with exact pass counts and the explicit remote-only gap for macOS ARM64 build verification.
- [ ] Commit as `test: verify public release readiness`.

## External Publication Gate

After all six tasks pass, stop and ask the user for the destination GitHub repository. Creating a public repository, adding a remote, pushing history, creating `v0.1.0`, and publishing the first Release require a separate final confirmation. The first tag should only be created after both root CI workflows pass on GitHub, especially the Apple Silicon macOS job.

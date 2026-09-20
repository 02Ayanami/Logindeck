# GitHub Public Release Design

## Status

Approved in conversation on 2026-09-21. This design prepares LoginDeck as a public-source learning
project and publishes unsigned Windows x64 and macOS Apple Silicon installation packages through
GitHub Releases.

## Goals

- Present LoginDeck as a conventional public GitHub repository with project files at the Git root.
- Run reproducible Windows and macOS verification on pull requests and the main branch.
- Build and publish one Windows installer and one Apple Silicon macOS installer from version tags.
- Keep the release process suitable for an unsigned learning project without adding store,
  notarization, certificate, or enterprise-browser-policy work.

## Repository Layout

The current outer Git repository contains the product under a redundant `LoginDeck/` directory.
All tracked product files move to the Git root. The existing outer Windows workflow and inner macOS
workflow move into one root `.github/workflows/` directory. Git moves preserve history; the project
must not be copied into a second nested tree.

The public root contains at least:

```text
.github/workflows/
app/
browser-extension/
crates/
docs/
scripts/
Cargo.toml
Cargo.lock
README.md
LICENSE
SECURITY.md
CONTRIBUTING.md
```

Generated directories, prepared Edge resources, package-manager stores, databases, credentials,
logs, local coordination workspaces, and release output remain ignored.

## License and Public Documentation

The project uses the MIT License with copyright holder `LoginDeck contributors`.

The root README is the GitHub landing page. It describes the product, supported systems, privacy
model, local build commands, unsigned-package warning, Edge manual-install boundary, and links to
contribution and security guidance. It must not claim code signing, notarization, Edge Add-ons
publication, Windows 10 live verification, Intel macOS support, or automatic Edge extension
installation.

`SECURITY.md` tells users not to publish passwords, credentials, database files, or vulnerability
details in public issues and provides a placeholder-free private reporting route based on GitHub
Security Advisories. `CONTRIBUTING.md` documents the pinned toolchains and platform verification
commands without requiring private infrastructure.

## Continuous Integration

The root workflows use least-privilege `contents: read` permissions for verification.

- Windows runs on `windows-2022` x64 and invokes the canonical
  `scripts/verify-windows.ps1` workflow from the repository root.
- macOS runs on GitHub's ARM64 `macos-14` runner and invokes the canonical
  `scripts/verify-macos.sh` workflow from the repository root.
- Both workflows run on pull requests to `main`, pushes to `main`, and manual dispatch.
- Package-manager and Rust cache keys include the correct root lockfiles after flattening.

The macOS verification workflow must prove Apple Silicon architecture explicitly and must not imply
Intel support. Existing platform-specific native tests retain their current environmental limits.

## Tagged Releases

A dedicated root release workflow triggers only for tags matching `v*` and by manual dispatch for
release-pipeline testing. It uses separate Windows and macOS jobs so each platform builds on its
native hosted runner.

The workflow validates that the tag version exactly matches `app/package.json`; mismatches fail
before publishing. Each platform installs the pinned Node, pnpm, and Rust toolchains and prepares the
bundled Edge resources through the existing build path.

Release assets are limited to:

- Windows x64 unsigned NSIS installer, renamed with product version and architecture.
- macOS Apple Silicon unsigned DMG, renamed with product version and architecture.
- `SHA256SUMS.txt` covering exactly those installers.

The MSI, raw/debug executable, `.app` directory, database, prepared unpacked extension, CI caches,
and test fixtures are not release assets. Jobs pass artifacts to one final release job; only that job
has `contents: write` permission and creates or updates the matching GitHub Release.

Release notes state that LoginDeck is learning software, binaries are unsigned, macOS supports Apple
Silicon only, Windows supports x64, and Edge extension loading follows the in-app guide. A failed
platform build prevents publication of a partial release.

## Security and Repository Audit

Before the first public push:

- scan all tracked current files for common private-key, token, password, and cloud-key patterns;
- inspect Git history for the same high-risk patterns rather than checking only `HEAD`;
- list tracked large files and reject generated binaries or personal data;
- confirm no database, system credential export, `.env`, API key, signing material, or local absolute
  path is shipped as source or a release asset;
- inspect all workflow permissions and pin action major versions from established publishers;
- verify the resulting root working tree and both canonical platform workflows.

If an actual secret is found in history, publishing pauses. The secret must be revoked first, then
history must be rewritten before creating the public remote. Documentation examples that contain no
working secret are not treated as credentials.

## GitHub Publication Boundary

The local repository currently has no Git remote. Repository restructuring and release automation
are completed and verified locally before any external GitHub action.

Creating the GitHub repository, selecting its public visibility, pushing source/history, creating a
tag, and publishing the first Release are external actions. They occur only after the user supplies
or selects the destination repository and confirms the final publication action. No force-push or
history rewrite is performed unless the audit proves it necessary and the user separately approves
the exact operation.

## Acceptance Criteria

- GitHub renders the root README and recognizes all workflows.
- The Git root contains no redundant product wrapper directory.
- MIT, security, and contribution files contain no placeholders.
- Windows verification still builds the NSIS installer successfully.
- macOS verification and ARM64 DMG build are represented by a valid workflow and later confirmed on
  GitHub's Apple Silicon runner; local Windows work must not claim that remote result in advance.
- A dry-run inspection proves release asset discovery, names, version matching, checksums, and
  least-privilege permissions.
- The public push is blocked if secret or personal-data review finds unresolved material.

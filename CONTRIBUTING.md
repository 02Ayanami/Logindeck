# Contributing to LoginDeck

LoginDeck is a learning project. Keep changes focused, preserve the local-only credential model, and
never commit real passwords, databases, tokens, certificates, or signing keys.

## Toolchain

- Rust 1.89.0, selected by `rust-toolchain.toml`
- Node.js 22
- pnpm 11.19.0, pinned in `app/package.json`
- Windows x64 or Apple Silicon macOS for native verification

Install frontend dependencies with:

```sh
corepack pnpm@11.19.0 --dir app install --frozen-lockfile
```

## Verification

On Windows x64:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-windows.ps1
```

On Apple Silicon macOS:

```sh
./scripts/verify-macos.sh
```

Before opening a pull request, run the canonical command for the platform you changed. Rust code
must pass `cargo fmt --all -- --check`; frontend changes must pass type checking and tests. Describe
platforms you did not run rather than implying they passed.

Do not add automatic login, automatic form submission, account switching, browser enterprise
policies, telemetry, or cloud credential storage without an approved design and explicit security
review. Edge extension installation remains a user action documented inside the app.

Contributions are provided under the repository's MIT License.

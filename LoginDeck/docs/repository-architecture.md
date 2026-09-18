# LoginDeck repository architecture

LoginDeck uses one product repository with a shared product layer and target-specific platform
implementations. The current verified target is macOS. A future Windows implementation belongs in
this repository, but it is added and enabled only when it can be compiled and tested on Windows.

## Layers

```text
app/                         React UI and Tauri desktop composition
crates/autologin-core/       Platform-neutral domain, repositories, services, and contracts
crates/platform-runtime/     Compile-time platform selection and stable native facade
crates/platform-macos/       macOS Keychain, clipboard, app catalog, icons, and accessibility
crates/platform-windows/     Reserved Windows implementation (not enabled yet)
crates/native-host/          Edge native-messaging process using the selected platform facade
browser-extension/           Shared Edge extension
```

Dependency direction is one way:

```text
frontend -> desktop commands -> autologin-core interfaces
                         \-> platform-runtime -> platform-macos
native-host ----------------^
```

`autologin-core` must not depend on Tauri or a concrete operating-system crate. Desktop commands
and the native host must import native types through `platform-runtime`, not directly from
`platform-macos`.

## Current macOS target

The macOS implementation owns all operating-system behavior:

- Login Keychain credential storage
- guarded clipboard writes and cleanup
- `.app` discovery, verification, and launch
- application icon extraction
- Accessibility-based login workflow support

The root verification entry point is:

```sh
./scripts/verify-macos.sh
```

The GitHub Actions workflow intentionally runs only on macOS while macOS is the only verified
target.

## Adding Windows later

Windows development should extend the existing repository rather than fork the frontend or core.
The expected sequence is:

1. Add `crates/platform-windows` implementing the existing core contracts.
2. Add a Windows selection branch and exports in `crates/platform-runtime`.
3. Add Windows-native packaging and verification scripts.
4. Add a Windows CI job only after it passes on a real Windows development machine.
5. Keep platform-specific behavior out of React and `autologin-core`.

Do not add placeholder Windows implementations from macOS. Platform code enters the main branch
only with target-native tests and a working build.

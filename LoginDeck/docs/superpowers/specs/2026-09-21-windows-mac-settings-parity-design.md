# Windows Settings and Edge Parity Design

## Status

Approved direction in conversation on 2026-09-21. This design uses the currently installed macOS
application as the product baseline: LoginDeck has a Settings destination anchored at the bottom of
the sidebar, with Language and Edge login detection controls. A later source cleanup removed that
destination; this work restores the macOS behavior and makes it functional on Windows rather than
showing a disabled imitation.

## Goal

Make the Windows application match the current macOS Settings experience while retaining one shared
React interface. Windows users can open Settings, change the application language, install the bundled
Edge extension and native host, enable or disable login detection, and observe the real connection
state.

## Scope

### Included

- Restore `settings` as the third shared application destination.
- Anchor Settings below Website accounts and Application accounts in the desktop sidebar.
- Restore the compact Settings page with exactly two product controls: Language and Edge login
  detection.
- Keep the existing bilingual installation and usage guide available from the Edge setting.
- Keep browser connection status separate from the enabled/disabled switch state.
- Install and register the bundled native messaging host for the current Windows user.
- Run the existing native host on Windows against the same application database and Windows
  Credential Manager backend as the desktop application.
- Preserve the current macOS implementation and behavior.

### Excluded

- Automatic filling, automatic login, account switching, UI Automation, adapters, and recorder work.
- Chrome, Chromium, Firefox, Beta, Dev, and Canary registration.
- Machine-wide installation under `HKEY_LOCAL_MACHINE`.
- Store publication, code signing, automatic extension-store installation, or enterprise-policy
  bypasses.
- Security, About, AI provider, API key, credential maintenance, or other unrelated settings cards.

## Product Baseline

The 2026-09-15 macOS verification record is authoritative for this feature. Settings contains
Language and an actual Edge detection switch; the switch defaults off and persists in SQLite. The
connection indicator reports a recent native connection and is not inferred from the switch. The
installation guide remains available from Settings.

The later website-header plugin shortcut is removed when the Settings destination is restored. This
avoids two competing homes for the same feature and matches the installed macOS navigation model.

## Shared Interface

`AppShell` exports `Page = 'websites' | 'applications' | 'settings'`. Websites and Applications stay
in their current order; Settings uses the existing gear icon and `sidebar__settings` class so it sits
at the bottom on wide layouts and becomes the third item in the compact navigation.

`App` renders a restored `SettingsPage`. The page uses the existing header and settings styles, the
existing `LanguageSetting`, and a focused `BrowserCaptureSetting` component. No operating-system
fork exists in React.

The Edge card shows:

- title and one-sentence description;
- enabled/disabled switch backed by `get_browser_capture_settings` and
  `set_browser_capture_enabled`;
- an independent connected/checking/disconnected indicator;
- an Installation and usage help action that opens the existing guide.

The switch is not optimistic after a failed save. It disables during the mutation, rolls back to the
last confirmed value on error, and renders only translated safe errors. Opening Settings triggers
installation preparation just as the macOS product does; failure does not enable detection or claim
a connection.

## Edge Resource Installation

The build continues to compile `autologin-native-host` in release mode, build the extension, and copy
both into the Tauri Edge resource directory. The host filename is platform-specific:
`autologin-native-host` on macOS and `autologin-native-host.exe` on Windows.

Installation remains per-user and repeatable. Shared validation verifies the bundled Manifest V3
extension identity, exact extension key, non-empty native host binary, and fixed host name
`com.autologin.native`. Platform-specific placement and registration are isolated behind the runtime
platform facade.

On Windows the installed layout is under the Tauri application data directory:

```text
%APPDATA%\com.autologin.desktop\edge-extension\...
%APPDATA%\com.autologin.desktop\native-host\autologin-native-host.exe
%APPDATA%\com.autologin.desktop\native-host\com.autologin.native.json
```

The host manifest contains an absolute executable path, `type: "stdio"`, and only the pinned
`chrome-extension://<LoginDeck extension id>/` origin. LoginDeck writes the default value of this
current-user registry key to the absolute manifest path:

```text
HKEY_CURRENT_USER\Software\Microsoft\Edge\NativeMessagingHosts\com.autologin.native
```

This follows Microsoft's current Native Messaging registration contract. The implementation does
not invoke `reg.exe`, PowerShell, a shell, or an installer subprocess. Registry access remains inside
`platform-windows` and uses explicit Windows APIs. See
[Microsoft Edge Native Messaging](https://learn.microsoft.com/en-us/microsoft-edge/extensions/developer-guide/native-messaging).

Existing matching registration is accepted and refreshed safely. A foreign or malformed value at
the owned host key is treated as `extension.setup_conflict`; it is never silently overwritten. Files
are staged with create-new semantics, flushed, and renamed into fixed application-owned locations.
Windows reparse points and unexpected file types in the owned path are rejected.

`open_edge_extensions` launches only the fixed `edge://extensions/` target through the platform
implementation. `open_edge_extension_folder` reveals only the installed extension directory. Neither
command accepts a frontend-supplied path or URL.

## Native Host Portability

The native host keeps the existing framed protocol, pinned extension-origin argument check, candidate
expiry, mutation serialization, revision invalidation, and secret-handling rules. The macOS-only
startup guard is removed.

The platform runtime exposes the application data directory for the fixed identifier
`com.autologin.desktop`. On Windows this resolves to `%APPDATA%\com.autologin.desktop`; on macOS it
continues to resolve to `~/Library/Application Support/com.autologin.desktop`. The desktop and native
host therefore open the same SQLite database and `desktop-instance.lock` on each platform.

The host already obtains `NativeCredentialStore` through `platform-runtime`; on Windows that selects
Credential Manager. No password is written to the registry, manifest, extension storage, logs, or
command output.

## Platform Boundaries

- Shared React and Tauri commands remain platform-neutral.
- Shared file validation and manifest creation may remain in the desktop integration layer.
- macOS path placement and process opening retain the current implementation.
- Windows registry, reparse-point checks, Edge process opening, and folder reveal code live in
  `platform-windows` and are selected through `platform-runtime`.
- `autologin-core` remains free of Tauri and concrete operating-system dependencies.

## Error Handling

Existing public error codes are reused:

- `extension.resources_missing` for missing or invalid bundled resources;
- `extension.setup_conflict` for an unexpected owned path or foreign registration;
- `extension.setup_failed` for filesystem or registry installation failures;
- `extension.edge_unavailable` when Edge or folder reveal cannot be opened;
- `platform.unsupported` only for compile targets other than the supported macOS and Windows builds.

Native OS error text and paths do not cross the command boundary. Failed installation leaves login
detection off unless it was already enabled from a previously valid installation.

## Security Invariants

- Registration is current-user only and targets one fixed host name.
- The manifest permits one pinned extension origin.
- No caller-controlled executable, registry key, path, URL, database path, service name, or
  credential namespace reaches the native host installer.
- The native host still refuses untrusted extension-origin arguments.
- Browser candidates remain memory-only until explicit user confirmation and expire under the
  existing protocol.
- Passwords remain in the native message only long enough for the existing zeroizing save flow.
- Windows installation must not follow reparse points or overwrite unrelated registrations/files.
- Existing macOS behavior and application identifier remain unchanged.

## Testing

### Frontend

- Settings appears as the third destination and reports `aria-current` correctly.
- Settings remains anchored at the bottom on desktop and is reachable in compact navigation.
- Language persistence and rollback tests continue to pass.
- Edge switch loads persisted state, saves once, blocks duplicate clicks, rolls back on failure, and
  never derives enabled state from connection state.
- Connection status covers checking, connected, disconnected, and polling cleanup.
- The guide opens from Settings; the website header no longer duplicates the plugin entry.
- English and Simplified Chinese keys remain identical.

### Rust and scripts

- Shared manifest validation accepts only the pinned extension build and host name.
- Windows installation tests use an isolated current-user registry fixture/key and temporary owned
  paths, and clean them after the test.
- Repeat installation is idempotent; foreign registration, reparse points, missing host, invalid
  manifest, and partial staging failures are rejected without overwriting unrelated state.
- Platform data-directory tests prove the desktop and native host resolve the same identifier path.
- The native host runs protocol/status/save tests with the Windows Credential Manager test namespace.
- Bundle tests assert the `.exe` filename and Tauri resource inclusion on Windows.
- Existing command allowlist remains exact; no new frontend command is required.

### End-to-end acceptance

On Windows 11 x64:

1. Open Settings and verify it visually matches the current macOS structure.
2. Change language, restart, and confirm persistence.
3. Prepare the Edge integration, load the unpacked extension, and check that Edge finds the native
   host through the HKCU registration.
4. Enable detection and confirm the desktop and extension report connected independently.
5. Submit a controlled HTTPS test login, approve it in the extension, and verify the account appears
   in LoginDeck with its password stored in Credential Manager.
6. Disable detection and verify a new candidate cannot be transferred or saved.
7. Restart both applications and repeat the connection check.

The canonical Windows verification script must pass afterward and rebuild EXE, MSI, and NSIS debug
artifacts. Windows 10 22H2 remains a required release target but must be reported as unverified until
run on a real Windows 10 host.

## Migration and Compatibility

No database migration is required: browser capture enabled state, revision, connection timestamp,
website records, and credentials already use shared storage contracts. Existing Windows users keep
their current account data. The initial setting stays off for databases without an explicit enabled
value.

The macOS installer path and native-host registration remain stable. Windows installation adds only
application-owned files and the one HKCU registration key described above.


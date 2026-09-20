# Windows Edge connection verification — 2026-09-21

## Automated result

Environment: Windows 11 x64, Rust 1.89.0 MSVC, Node.js 22, pnpm 11.19.0.

`powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-windows.ps1` passed.
The run rebuilt the Edge extension and release native host, passed 246 Rust tests (2 controlled
tests ignored), 8 Node script tests, and 66 frontend tests, then completed typecheck, production
frontend build, and Tauri debug EXE/MSI/NSIS packaging.

Artifacts:

- `target/debug/autologin-desktop.exe`
- `target/debug/bundle/msi/LoginDeck_0.1.0_x64_en-US.msi`
- `target/debug/bundle/nsis/LoginDeck_0.1.0_x64-setup.exe`

The rebuilt desktop was launched successfully as a responsive `LoginDeck` window. After the user
loaded the unpacked extension and enabled Edge login detection, system-side verification found the
desktop and `autologin-native-host.exe` running concurrently (native host PID 119352).

## Production registration smoke test

The opt-in production installer smoke test passed using the bundled resources. The exact current-user
key is `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.autologin.native`. Its default value
points to:

`C:\Users\<user>\AppData\Roaming\com.autologin.desktop\native-host\com.autologin.native.json`

The parsed manifest has name `com.autologin.native`, type `stdio`, exactly one allowed origin
(`chrome-extension://afklnhhancnomdifldifpmdgklflbigi/`), and an existing absolute executable at
`%APPDATA%\com.autologin.desktop\native-host\autologin-native-host.exe`. The installed extension
manifest also exists under `%APPDATA%\com.autologin.desktop\edge-extension`.

An older LoginDeck-owned registration pointing to `%LOCALAPPDATA%` was removed after its fixed host
name and pinned extension origin were verified. The old files were not deleted. The new installer
then created the roaming-data registration successfully.

## Remaining live acceptance

Loading the unpacked extension into stable Edge and establishing the real native connection passed.
Saving a synthetic HTTPS credential through the confirmation flow, checking Windows Credential
Manager, and verifying the disabled-state rejection remain pending because they require a controlled
test website/login fixture. The available Windows UI-control channel did not expose native
application windows in this run.

Release gaps remain unchanged for Edge Add-ons publication, code signing, and a real Windows 10
22H2 run.

## NSIS installation acceptance

After explicit user confirmation, `LoginDeck_0.1.0_x64-setup.exe /S` completed with exit code 0.
The installer created the current-user installation at
`C:\Users\<user>\AppData\Local\LoginDeck`, an uninstaller, an uninstall-registry entry reporting
version `0.1.0`, and the current-user Start Menu shortcut `LoginDeck.lnk`.

The installed application contains both `edge\extension\manifest.json` and
`edge\autologin-native-host.exe`. The installed executable launched with a responsive `LoginDeck`
window while the Edge native-host process remained active. A stop/start cycle produced another
responsive installed window; the roaming SQLite database remained present at 106496 bytes and the
fixed Edge registration remained present.

The installer was executed but not code-signed. Uninstall behavior remains untested because the
user retained the installed application for continued use. Windows 10 execution remains unverified.

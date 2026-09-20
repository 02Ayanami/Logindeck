# Edge in-app installer verification

- Frontend: 88 tests passed, including installation success, fixed browser/folder actions, missing-resource failure and retry. Installing does not enable detection.
- Desktop Rust: 9 unit tests and 3 command-contract tests passed. Installer tests cover repeat installation, removal of obsolete extension files, stable paths, exact extension allowlist, missing resources and preservation of an unrelated registration.
- Desktop Clippy with warnings denied passed. Frontend and macOS app packaging passed.
- Inspected the actual app bundle: it contains edge/autologin-native-host, complete extension files and icon.icns. Native host bytes match the staged resource.
- Real UI: Settings → Install Edge extension automatically prepared the stable extension folder and native registration. Installed host bytes match the bundled executable.
- Real UI: Open Edge extensions opened the Edge extensions management page. Show extension folder opened Finder with edge-extension selected. The installer page rendered the screenshots and selectable folder path.
- No account records or detection preferences were changed. Detection was already enabled when this verification began and was left enabled.
- Loading the stable folder in Edge remains a browser confirmation step; this verification did not replace the user's existing source-folder extension installation or repeat a website login/save test.

# Settings and Edge control verification — 2026-09-15

## Delivered

- Settings contains language and an actual Edge detection switch, default off, stored in SQLite. About, security explanations and maintenance UI are removed. Automatic credential cleanup is retained.
- The switch revision invalidates pending captures across off/on transitions. The native vault checks enabled state and revision under the same mutation lock as saving and setting changes. Disabling does not remove existing accounts.
- Visible HTTPS content pages poll only non-secret status, stop extraction while disabled, and fail closed if status becomes stale. The worker checks state again before transferring a candidate. The native host rejects disabled saves and discards pending secrets.
- Connection check feedback is separate from candidate status. Progress, success, failure and missing replies are covered. Current desktop status reflects a recent native connection; it is not inferred from the switch.
- Dedicated bilingual help page with four installation/use steps, troubleshooting and four cropped screenshots from actual Edge/LoginDeck UI. Developer commands are under an advanced disclosure.
- Sidebar settings is anchored at the bottom. App UI, platform icons and extension icons derive from app/src/assets/logindeck.svg.

## Automated verification

- Frontend: 86 tests pass, including switch persistence calls, save failure, independent help navigation and Chinese help.
- Extension: 6 tests pass, including repeated connection feedback, invalid/missing response handling, disabled capture rejection and stale revision rejection.
- Rust workspace: 101 regular tests plus documentation tests pass. Two tests are marked ignored for their existing special execution requirements (the child lock probe is exercised by its parent test).
- Clippy with -D warnings passes. Production frontend and debug macOS app bundle build successfully. git diff --check passes.

## Native UI checks

- Launched the rebuilt LoginDeck.app; settings shows only language and Edge detection. Initial switch off.
- Switched on/off through the UI; labels changed correctly. Restarted the application; off persisted. Final state off, original Chinese locale retained.
- Confirmed the compact settings layout fits the default 800×600 window and settings remains at the sidebar bottom.
- Loaded Edge extension version 0.2.0 and updated its installed native component. The actual extension popup displayed “已连接 LoginDeck” separately from “登录识别已关闭”. Desktop showed recent connection.
- Opened the independent help page, scrolled to verify embedded screenshots rendered, and returned to settings. Help text and screenshots contain no real credentials.
- Closed the dedicated extension test window. No website/account records were created or modified in this round.

This round did not repeat a real website login/save transaction. Pending-candidate disable, stale confirmation, save/update and conflict behavior were exercised by automated native/extension tests rather than by signing into a real account.

## Follow-up: packaged application icon

The initial icon verification above was incomplete: source icons and the extension were updated, but `bundle.icon` was absent from the Tauri configuration. The resulting macOS app had neither `CFBundleIconFile` nor a Resources/icon.icns file.

Added explicit bundle icon paths and rebuilt. Verified `CFBundleIconFile = icon.icns` and the bundled resource matches the source ICNS byte for byte. Refreshed this app's Launch Services registration and restarted its exact bundle path. Read back both NSWorkspace's file icon and NSRunningApplication's icon; both visibly show the new blue cards/keyhole mark. No global Dock or Finder cache reset was needed.

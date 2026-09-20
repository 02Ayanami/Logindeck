# LoginDeck confirmed product design

2026-09-17. Implementation reference agreed with the user. Do not package yet.

- Separate Website accounts and Application accounts navigation. No settings destination.
- Dark narrow sidebar, light neutral canvas, white grouped cards, restrained blue actions.
- Each target shows three accounts initially; expand remaining accounts inline. Search target names, origins and account labels/identifiers.
- Account display name is a local remark, separate from the real login identifier. Empty name fields show the next Account N as a placeholder and persist that default on save. Counters are scoped to targets and never reused after deletion.
- Target header actions: add account, open target; icon buttons. Copy identifier directly beside the identifier, then copy password. More menu contains edit/delete account.
- Add/edit in a right drawer. Password blank on edit preserves the existing password.
- Website groups exist only while they have accounts. No website bulk deletion. Last account deletion removes the group. Website metadata can be edited.
- Application management: scan new apps, manually add, inspect metadata, remove empty applications. Applications with accounts must have their accounts deleted individually first; no bulk deletion. Scanning is additive, never removes existing applications/accounts. No routine presence badges.
- No automatic login, fill, switching, SMS, adapters, recorder or accessibility setup in the product.
- Website capture: only while connected; capture on form submission, ask consent before sending credentials. No preflight account lookup. LoginDeck matches exact real login identifier and normalized origin; creates or updates, preserves local display names. Idempotent request IDs.
- Only created_account / updated_account results; format: GitHub · 已新增账号 1 / GitHub · 已更新个人账号的密码.
- LoginDeck provides installation files and full instructions; browser extension offers connection detection. Extension follows LoginDeck language.
- Page capture card top right, consent expires after 15 seconds, success after 4 seconds. Dismiss discards transient credentials. Clicking outside does not dismiss. One prompt per submission across navigation.
- No maintenance panel. Cross-store operations report success only after completion; failed credential deletion retains account metadata. Durable internal recovery is still necessary for crashes.

Implementation verification must cover migrations, grouped account identity, naming counters, additive scans, copy/open operations, capture consent and result feedback, and deletion failures. Use development previews, no packaging.

## Verification on 2026-09-17

- Implemented grouped website/application pages, management view, shared account drawer and plugin guide.
- TypeScript, 61 frontend tests, 6 extension tests and Rust workspace tests passed. Vite and unpacked extension builds passed; no installer was built.
- Reviewed website/application/management/plugin layouts in the development preview, including the 720 × 540 minimum window. Preview uses synthetic data and mocked native commands.
- Real Edge → native host → macOS Keychain end-to-end verification remains outstanding. Automated credential tests use test stores and do not establish live Keychain behavior.
- Account deletion journals recover interrupted operations; credential-delete errors retain account metadata. Pending deletions cannot be edited. This is recovery across two stores, not an atomic Keychain/SQLite transaction.

## Development cleanup

- Removed unused settings/capture-toggle/maintenance pages and superseded editors, including tests dedicated to those removed screens.
- Consolidated installation and usage instructions into the active plugin guide; removed obsolete help about toggles and 60-second prompts.
- Added explicit incomplete-save feedback for storage errors, retained silent dismissal on disconnect, and clear the page prompt when connection polling fails.
- Blank names on existing application accounts preserve the local remark, matching website edits. Edit placeholders now show the retained name.
- TypeScript/Vite, extension syntax/build and Rust workspace compilation checked. Live integration remains deferred until development is ready for the final test pass.

## Final scope and acceptance update — 2026-09-17

This update supersedes the earlier outstanding-integration notes above. Real Edge capture verification is recorded in `testing/2026-09-17-edge-live-redesign.md`. The user subsequently confirmed that the connection indicator works and that installation instructions match actual operation. Those confirmations are user acceptance, not an additional agent-run live test.

The plugin drawer now contains the single-sentence description and a four-step illustrated tutorial directly. Files are prepared automatically; step 2 displays the actual folder path. The website header shows a red/green connection indicator immediately before the plugin icon. Historical automation and recorder tools are outside the product scope. See `testing/2026-09-17-release-readiness.md` for the current version and verification summary. No packaging or release has been performed.

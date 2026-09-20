# Code review fixes

- Password replacement reports success once the new credential and metadata are committed. Failure to retire the old credential remains in the durable cleanup queue and is retried; it does not falsely report that the new save failed. Applies to website and application accounts. Account deletion still requires credential deletion before reporting success.
- Account numbering is now committed in the same metadata transaction as account creation. Validation, credential-write or database failures do not consume a number. Service mutation locking covers the number lookup and commit.
- Migration 0009 repairs legacy application counters without modifying applied migration checksums. Number lookup also checks existing Chinese/English numbered labels, and committed high watermarks survive account deletion.
- capture_source records the latest write source: manual edits become manual, browser updates become browser_extension. Editing a browser-saved account's remark no longer produces a false browser password-update notice; later browser updates preserve the remark.
- Regression coverage: failed-create numbering, committed-save cleanup/replay/recovery, manual rename source and browser remark preservation, application high labels/failed creates/deletion high watermark. The legacy cleanup contract test now expects successful committed saves.
- Migration verified using an isolated SQLite database with Chinese/English labels and an existing higher counter. No real account data modified during this fix.

## Follow-up boundary fixes

- A disconnected status response or failed status request clears all transient candidates immediately, even if the native messaging process remains alive. Reconnection cannot restore an earlier prompt or confirm its old candidate ID.
- Website write timestamps retain epoch-second format with nine fractional digits and advance beyond the preceding record timestamp even if the system clock repeats or moves backward. Legacy whole-second values remain accepted.
- Regression coverage includes reconnecting before candidate expiry, rejecting the discarded candidate ID, ten consecutive browser updates to one account, legacy timestamps and clock rollback.

## Desktop capture feedback

- Process every changed browser account in a refresh, showing one notice per account and highlighting all affected rows. Expand each affected website group.
- Track each account's feedback expiry independently. A further update to the same account restarts its four-second notice/highlight without extending unrelated notices.
- Frontend regression uses four simultaneous changes, another update after three seconds, and checks old notices expire at four seconds while the renewed notice remains until seven seconds.

## Consolidated asynchronous refresh review

- Website and application list requests carry a monotonically increasing request version. Only the current request can replace data, report an error or clear loading state.
- Unmount/effect cleanup and mutation start invalidate previous list requests. This prevents obsolete results from resurfacing removed accounts or showing errors from an earlier page lifetime.
- Added website response/error reordering regression and application StrictMode effect-restart regression. Reviewed scan polling (existing request/phase guards), icon loading (existing effect cleanup), and shared account-drawer default-name loading (existing effect cleanup).
- Frontend suite: 55 tests passed; TypeScript and Vite build passed. Extension suite: 6 tests passed. No real-account mutations or packaging performed.

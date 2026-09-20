# AutoLogin AI Desktop Login Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add safe, general-purpose, AI-guided one-click login for macOS applications without per-application adapters.

**Architecture:** The shared core owns provider contracts, screenshot-analysis schemas, template matching, and a deterministic login state machine. The macOS platform crate captures only the chosen application window, verifies window identity, and emits bounded input events; cloud models only return proposed controls and never receive credentials or gain local control.

**Tech Stack:** Existing Tauri/React/Rust workspace, reqwest, serde/JSON Schema, Gemini API, optional OpenAI/OpenAI-compatible providers, ScreenCaptureKit, Accessibility API, Core Graphics Events, image hashing, Vitest and Cargo tests.

**Spec:** `docs/superpowers/specs/2026-09-11-autologin-macos-design.md`

## Global Constraints

- This plan starts only after `2026-09-11-autologin-desktop-foundation.md` is complete and passing.
- Default provider is Gemini with model `gemini-2.5-flash-lite`; users supply their own API key.
- Provider, Base URL, model, timeout, and confidence threshold are configurable; the app never silently switches to a paid model.
- Only the selected target-application window may be captured; no full-desktop capture API is exposed to React.
- Control-location screenshots are created before credentials are entered. Post-submit screenshots must locally redact all known credential regions before upload.
- Passwords and API keys remain in `CredentialStore`, never enter AI requests, logs, frontend events, temporary files, or screenshot caches.
- New window templates require a user-confirmed overlay; unchanged confirmed templates may support one-click login.
- Already-logged-in, QR, verification, unknown, low-confidence, changed-window, or changed-focus states stop before password input.
- AI responses are untrusted suggestions and must pass JSON Schema, geometry, confidence, application identity, window identity, and accessibility checks.
- Automatic logout and automatic completion of MFA are out of scope.

---

## File Structure

```text
crates/autologin-core/src/ai/              # provider-neutral analysis types and validation
crates/autologin-core/src/login/           # state machine and orchestration
crates/autologin-core/src/templates.rs      # confirmed template matching
crates/autologin-providers/                 # Gemini/OpenAI-compatible HTTP clients
crates/platform-macos/src/window_capture.rs # selected-window capture
crates/platform-macos/src/window_guard.rs   # process/window/focus verification
crates/platform-macos/src/input.rs          # bounded mouse/keyboard events
app/src-tauri/src/commands/ai_settings.rs   # provider settings and connection test
app/src-tauri/src/commands/login.rs         # start/cancel/status commands
app/src/features/settings/AiSettings.tsx    # provider setup
app/src/features/login/                     # preview, confirmation, progress, errors
tests/fixtures/login-screens/               # synthetic, credential-free images
```

## Task 1: Define AI analysis types and strict validation

**Files:**
- Create: `crates/autologin-core/src/ai/mod.rs`
- Create: `crates/autologin-core/src/ai/schema.rs`
- Create: `crates/autologin-core/src/ai/validate.rs`
- Modify: `crates/autologin-core/src/lib.rs`
- Test: `crates/autologin-core/tests/analysis_validation.rs`

**Interfaces:**
- Produces: `ScreenState`, `NormalizedRect`, `DetectedControl`, `LoginScreenAnalysis`, and `ValidatedLoginControls`.
- Produces: `validate_analysis(analysis, threshold) -> Result<ValidatedLoginControls, AppError>`.

- [ ] **Step 1: Write validation tests**

```rust
#[test]
fn accepts_ordered_high_confidence_login_controls() {
    let analysis = fixture_analysis(ScreenState::LoginForm, 0.96);
    let controls = validate_analysis(analysis, 0.90).unwrap();
    assert!(controls.username.center().y < controls.password.center().y);
}

#[test]
fn rejects_qr_and_unknown_states_before_geometry_use() {
    for state in [ScreenState::QrLogin, ScreenState::Verification, ScreenState::Unknown] {
        assert_eq!(validate_analysis(fixture_analysis(state, 0.99), 0.90).unwrap_err().code(), "login.unsupported_screen");
    }
}

#[test]
fn rejects_out_of_bounds_or_low_confidence_controls() {
    assert_eq!(validate_analysis(out_of_bounds_fixture(), 0.90).unwrap_err().code(), "ai.invalid_geometry");
    assert_eq!(validate_analysis(fixture_analysis(ScreenState::LoginForm, 0.50), 0.90).unwrap_err().code(), "ai.low_confidence");
}
```

- [ ] **Step 2: Run tests and verify failure**

Run: `cargo test -p autologin-core --test analysis_validation`

Expected: FAIL because analysis types and validation do not exist.

- [ ] **Step 3: Implement types and validation**

Use normalized `f64` coordinates in `[0.0, 1.0]`. Require finite values, positive rectangles, centers inside the window, username/password separation, plausible reading order, and three controls at or above threshold. Deserialize with `deny_unknown_fields`. Keep `reason` and `warnings` length-limited and treat them as display-only untrusted text.

```rust
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetectedControl { pub rect: NormalizedRect, pub confidence: f64, pub reason: String }

pub fn validate_analysis(a: LoginScreenAnalysis, threshold: f64) -> Result<ValidatedLoginControls, AppError> {
    if a.screen_state != ScreenState::LoginForm { return Err(AppError::new("login.unsupported_screen")); }
    validate_controls(a, threshold)
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p autologin-core --test analysis_validation && cargo test -p autologin-core`

Expected: PASS for valid, malformed, missing, extra-field, NaN, overlap, out-of-bounds, and low-confidence fixtures.

- [ ] **Step 5: Commit**

```bash
git add crates/autologin-core
git commit -m "feat: validate AI login-screen analysis"
```

## Task 2: Implement provider configuration and Gemini client

**Files:**
- Create: `crates/autologin-providers/Cargo.toml`
- Create: `crates/autologin-providers/src/lib.rs`
- Create: `crates/autologin-providers/src/gemini.rs`
- Create: `crates/autologin-providers/src/openai_compatible.rs`
- Create: `crates/autologin-core/src/ai/provider.rs`
- Modify: `Cargo.toml`
- Test: `crates/autologin-providers/tests/provider_contract.rs`
- Test: `crates/autologin-providers/tests/fixtures/`

**Interfaces:**
- Produces: `AiProvider::analyze_login_window(request) -> Result<LoginScreenAnalysis, AppError>`.
- Produces: `ProviderConfig { kind, base_url, model, timeout_ms, confidence_threshold, secret_ref }`.
- Produces: `ProviderRegistry::from_config(config, credential_store)`.

Add `crates/autologin-providers` to the root Cargo workspace and set its package name to `autologin-providers` before running this task's tests.

- [ ] **Step 1: Write HTTP-contract tests with a local mock server**

```rust
#[tokio::test]
async fn gemini_request_contains_image_and_schema_but_no_credentials() {
    let server = MockProvider::gemini(valid_analysis_json());
    let provider = GeminiProvider::new(server.config(), secret("api-key"));
    let result = provider.analyze_login_window(sample_request()).await.unwrap();
    let body = server.single_request_body();
    assert!(body.contains("inline_data"));
    assert!(body.contains("username_field"));
    assert!(!body.contains("alice@example.com"));
    assert!(!body.contains("account-password"));
    assert_eq!(result.screen_state, ScreenState::LoginForm);
}

#[tokio::test]
async fn billing_or_quota_errors_never_trigger_provider_fallback() {
    let provider = provider_returning_status(429);
    assert_eq!(provider.analyze_login_window(sample_request()).await.unwrap_err().code(), "ai.quota_exceeded");
}
```

- [ ] **Step 2: Run tests and verify failure**

Run: `cargo test -p autologin-providers --test provider_contract`

Expected: FAIL because provider crate and clients do not exist.

- [ ] **Step 3: Implement the provider trait and clients**

Use a fixed system prompt that asks only for the schema fields in the spec. Send the screenshot as an in-memory encoded image, set structured JSON response configuration, cap response bytes, disable retries for authentication/billing errors, and apply a total timeout. Implement OpenAI and custom OpenAI-compatible requests behind the same trait. Validate custom Base URLs as HTTPS, allowing HTTP only for loopback addresses after an explicit advanced-setting confirmation.

```rust
#[async_trait::async_trait]
impl AiProvider for GeminiProvider {
    async fn analyze_login_window(&self, request: AnalysisRequest) -> Result<LoginScreenAnalysis, AppError> {
        let response = self.client.post(self.endpoint()).timeout(self.timeout).json(&self.body(request)).send().await?;
        map_status(response.status())?;
        decode_bounded_analysis(response, 64 * 1024).await
    }
}
```

- [ ] **Step 4: Add provider configuration persistence**

Store non-secret provider fields in `settings`; store the API key through `CredentialStore`. Default a new profile to Gemini and `gemini-2.5-flash-lite` with no key. A configuration update must not modify the saved key unless the request includes a replacement or explicit deletion.

```rust
pub enum ApiKeyUpdate { Unchanged, Replace(SecretString), Delete }
pub struct SaveProviderConfig { pub kind: ProviderKind, pub base_url: Url, pub model: String, pub api_key: ApiKeyUpdate }
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p autologin-providers && cargo test --workspace`

Expected: PASS for Gemini, OpenAI-compatible, malformed JSON, oversized response, invalid URL, timeout, authentication, quota, and billing fixtures.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/autologin-core crates/autologin-providers migrations
git commit -m "feat: add configurable cloud vision providers"
```

## Task 3: Capture and redact only the target application window

**Files:**
- Modify: `crates/autologin-core/src/platform.rs`
- Create: `crates/platform-macos/src/window_capture.rs`
- Create: `crates/platform-macos/src/redaction.rs`
- Test: `crates/platform-macos/tests/window_capture.rs`
- Test: `crates/platform-macos/tests/redaction.rs`

**Interfaces:**
- Produces: `WindowCapture::front_window(app) -> WindowSnapshot`.
- Produces: `WindowSnapshot { identity, bounds, scale_factor, rgba, captured_at }` with image bytes omitted from `Debug`.
- Produces: `redact(snapshot, rects) -> RedactedImage`.

- [ ] **Step 1: Write redaction and authorization tests**

```rust
#[test]
fn redaction_replaces_every_pixel_inside_sensitive_rect() {
    let image = solid_test_image(100, 100, [255, 0, 0, 255]);
    let output = redact_rgba(image, &[PixelRect::new(10, 10, 20, 20)]).unwrap();
    assert_eq!(output.pixel(15, 15), [32, 32, 32, 255]);
    assert_eq!(output.pixel(5, 5), [255, 0, 0, 255]);
}

#[tokio::test]
async fn capture_rejects_window_owned_by_another_process() {
    let capture = fixture_capture_with_mismatched_owner();
    assert_eq!(capture.front_window(&target_app()).await.unwrap_err().code(), "window.owner_mismatch");
}
```

- [ ] **Step 2: Run tests and verify failure**

Run: `cargo test -p platform-macos --test redaction --test window_capture`

Expected: FAIL because capture and redaction are not implemented.

- [ ] **Step 3: Implement ScreenCaptureKit capture**

Request screen-recording permission only from an explicit user action. Enumerate windows, filter by verified PID/Bundle Identifier, exclude desktop and other applications, and capture exactly one chosen window. Apply a maximum dimension before provider encoding, preserve coordinate mapping metadata, and never write bytes to disk.

```rust
let target = windows.into_iter().find(|w| w.owner_pid == app.pid && w.is_frontmost)
    .ok_or_else(|| AppError::new("window.not_found"))?;
self.screen_capture_kit.capture_window(target.window_id, MAX_IMAGE_EDGE).await
```

- [ ] **Step 4: Implement deterministic local redaction**

Expand each known credential rectangle by a safety margin, clamp it to image bounds, fill it with an opaque neutral color, and zero the original byte buffer after encoding. Return an error if a requested rectangle is invalid rather than uploading an incompletely redacted image.

```rust
for rect in rects { image.fill(rect.expand(REDACTION_MARGIN)?.clamp(image.bounds())?, [32, 32, 32, 255]); }
Ok(RedactedImage::new(image))
```

- [ ] **Step 5: Run tests and manual permission check**

Run: `cargo test -p platform-macos --test redaction --test window_capture && cargo test --workspace`

Manual check: capture a disposable test app while another private window is visible; verify only the selected test window appears in the preview.

- [ ] **Step 6: Commit**

```bash
git add crates/autologin-core crates/platform-macos
git commit -m "feat: capture and redact target app windows"
```

## Task 4: Persist and invalidate user-confirmed login templates

**Files:**
- Create: `crates/autologin-core/src/templates.rs`
- Modify: `crates/autologin-core/src/sqlite.rs`
- Test: `crates/autologin-core/tests/template_matching.rs`

**Interfaces:**
- Produces: `TemplateService::candidate(application_id, snapshot) -> TemplateDecision`.
- Produces: `TemplateDecision::{ConfirmedMatch, NeedsAnalysis, Rejected}`.
- Produces: `confirm_template(application_id, snapshot, controls, provider, model)`.

- [ ] **Step 1: Write template matching tests**

```rust
#[tokio::test]
async fn confirmed_similar_window_skips_cloud_analysis() {
    let service = fixture_template_service_with_confirmed_login();
    assert!(matches!(service.candidate(app_id(), similar_snapshot()).await.unwrap(), TemplateDecision::ConfirmedMatch(_)));
}

#[tokio::test]
async fn changed_layout_requires_new_analysis_and_confirmation() {
    let service = fixture_template_service_with_confirmed_login();
    assert!(matches!(service.candidate(app_id(), changed_layout_snapshot()).await.unwrap(), TemplateDecision::NeedsAnalysis));
}
```

- [ ] **Step 2: Run tests and verify failure**

Run: `cargo test -p autologin-core --test template_matching`

Expected: FAIL because template service does not exist.

- [ ] **Step 3: Implement matching and persistence**

Build a `window_signature` from platform application ID, normalized title class, size class, accessibility summary when available, and a credential-free perceptual hash. Persist only signature, normalized rectangles, provider/model metadata, confidence summary, and confirmation time. Never persist screenshot bytes or thumbnails. Set conservative similarity thresholds and return `NeedsAnalysis` on uncertainty.

```rust
match self.repo.find_confirmed(app_id).await? {
    Some(t) if t.signature.matches(snapshot, self.thresholds) => Ok(TemplateDecision::ConfirmedMatch(t.controls)),
    _ => Ok(TemplateDecision::NeedsAnalysis),
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p autologin-core --test template_matching && cargo test --workspace`

Expected: PASS for exact match, small scale change, theme change, title change, major layout change, different application, and unconfirmed template.

- [ ] **Step 5: Commit**

```bash
git add crates/autologin-core migrations
git commit -m "feat: cache confirmed login window templates"
```

## Task 5: Implement window guards and bounded input

**Files:**
- Modify: `crates/autologin-core/src/platform.rs`
- Create: `crates/platform-macos/src/window_guard.rs`
- Create: `crates/platform-macos/src/input.rs`
- Test: `crates/platform-macos/tests/input_guard.rs`

**Interfaces:**
- Produces: `WindowInspector::verify(expected) -> VerifiedWindow`.
- Produces: `InputController::click(verified, point)` and `type_secret(verified, SecretString)`.
- Consumes: only `ValidatedLoginControls`, never raw AI rectangles.

- [ ] **Step 1: Write fail-closed guard tests**

```rust
#[tokio::test]
async fn input_stops_when_focus_changes_between_steps() {
    let harness = InputHarness::focus_changes_after_first_click();
    harness.click_username().await.unwrap();
    let error = harness.type_username("alice").await.unwrap_err();
    assert_eq!(error.code(), "window.focus_changed");
    assert_eq!(harness.typed_event_count(), 0);
}

#[tokio::test]
async fn coordinate_outside_verified_window_is_rejected() {
    let error = fixture_controller().click(&verified_window(), Point::new(-1.0, 40.0)).await.unwrap_err();
    assert_eq!(error.code(), "input.out_of_bounds");
}
```

- [ ] **Step 2: Run tests and verify failure**

Run: `cargo test -p platform-macos --test input_guard`

Expected: FAIL because guarded input does not exist.

- [ ] **Step 3: Implement window verification**

Verify PID, Bundle Identifier, code signature, window ID, bounds, title class, foreground status, and capture age. Revalidate immediately before every click and every typed field. If Accessibility exposes control roles, reject conflicts such as an AI password rectangle mapping to a non-editable control.

```rust
pub async fn verify(&self, expected: &ExpectedWindow) -> Result<VerifiedWindow, AppError> {
    let current = self.snapshot_identity(expected.window_id).await?;
    if current != expected.identity { return Err(AppError::new("window.changed")); }
    Ok(VerifiedWindow::new(current))
}
```

- [ ] **Step 4: Implement bounded click and typing**

Generate Core Graphics events only for points inside the verified window. Type Unicode through a deterministic event path without the clipboard. Insert configurable short delays, abort through a cancellation token, and zero secret buffers after use. Never provide a public method that accepts an arbitrary process or unverified screen point.

```rust
pub async fn click(&self, window: &VerifiedWindow, point: Point) -> Result<(), AppError> {
    if !window.bounds().contains(point) { return Err(AppError::new("input.out_of_bounds")); }
    self.events.left_click(point).await
}
```

- [ ] **Step 5: Run tests and a disposable-app smoke test**

Run: `cargo test -p platform-macos --test input_guard && cargo test --workspace`

Manual check: use a local test form containing username and secure-password fields; verify switching focus to another application before password entry stops with no password characters emitted.

- [ ] **Step 6: Commit**

```bash
git add crates/autologin-core crates/platform-macos
git commit -m "feat: guard macOS login input events"
```

## Task 6: Build the deterministic login state machine

**Files:**
- Create: `crates/autologin-core/src/login/state.rs`
- Create: `crates/autologin-core/src/login/orchestrator.rs`
- Create: `crates/autologin-core/src/login/events.rs`
- Test: `crates/autologin-core/tests/login_state_machine.rs`

**Interfaces:**
- Produces: states from `Idle` through `Succeeded`, `AwaitingUserVerification`, `Failed`, and `Cancelled` exactly as specified.
- Produces: `LoginOrchestrator::start(account_id) -> LoginTaskId` and `cancel(task_id)`.
- Emits: non-secret `LoginEvent { task_id, state, error_code, params }`.

- [ ] **Step 1: Write transition-table tests**

Test the complete happy path plus installation failure, missing permission, template match, new-template confirmation, rejected template, provider timeout, QR state, already logged in, user-presence denial, focus change before username, focus change before password, cancel during each await, and second concurrent task rejection.

```rust
#[tokio::test]
async fn qr_screen_never_reads_the_password() {
    let harness = LoginHarness::screen_state(ScreenState::QrLogin);
    let result = harness.run().await.unwrap_err();
    assert_eq!(result.code(), "login.unsupported_screen");
    assert_eq!(harness.credentials.reveal_count(), 0);
    assert_eq!(harness.input.event_count(), 0);
}
```

- [ ] **Step 2: Run tests and verify failure**

Run: `cargo test -p autologin-core --test login_state_machine`

Expected: FAIL because orchestrator and states do not exist.

- [ ] **Step 3: Implement the state machine**

Encode allowed transitions explicitly rather than as ad hoc booleans. Acquire the single-task lock before launch. Delay credential reveal until after analysis validation and template confirmation. Check cancellation before and after every awaited operation. Treat submit as non-idempotent and never retry it automatically.

```rust
let _guard = self.single_task.try_lock().map_err(|_| AppError::new("login.already_running"))?;
self.transition(LoginState::Validating)?;
let controls = validate_analysis(analysis, settings.confidence_threshold)?;
self.await_template_confirmation_if_needed(&controls).await?;
let password = self.credentials.reveal(&account.secret_ref, "Sign in").await?;
```

- [ ] **Step 4: Implement post-submit observation**

Clear password memory before observing. Prefer local Accessibility state; if cloud analysis is needed, redact credential rectangles before encoding. Map verification pages to `AwaitingUserVerification`, send a notification, and stop emitting input events.

```rust
drop(password);
let observed = self.observer.observe(redaction_rects).await?;
if observed == ScreenState::Verification {
    self.transition(LoginState::AwaitingUserVerification)?;
    self.notifier.verification_required(&app.display_name).await?;
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p autologin-core --test login_state_machine && cargo test --workspace`

Expected: PASS and all failure paths show zero input after terminal state.

- [ ] **Step 6: Commit**

```bash
git add crates/autologin-core
git commit -m "feat: orchestrate safe AI-guided login"
```

## Task 7: Expose AI settings and login commands

**Files:**
- Create: `app/src-tauri/src/commands/ai_settings.rs`
- Create: `app/src-tauri/src/commands/login.rs`
- Modify: `app/src-tauri/src/lib.rs`
- Test: `app/src-tauri/tests/login_command_contract.rs`

**Interfaces:**
- Produces commands: `get_ai_settings`, `save_ai_settings`, `delete_ai_key`, `test_ai_connection`, `start_login`, `confirm_template`, `reject_template`, `cancel_login`, and `get_login_status`.
- Emits: `login://state` events without screenshots or secrets.

- [ ] **Step 1: Write command security tests**

Verify masked key output, test-image use for connection checks, absence of secrets in events/errors, no provider fallback on quota errors, rejection of concurrent tasks, and rejection of template confirmation for another task ID.

```rust
#[tokio::test]
async fn ai_settings_never_return_the_full_key() {
    let output = harness_with_key("secret-api-key").get_ai_settings().await.unwrap();
    assert!(output.has_api_key);
    assert_eq!(output.api_key_suffix, "-key");
    assert!(!serde_json::to_string(&output).unwrap().contains("secret-api-key"));
}
```

- [ ] **Step 2: Run tests and verify failure**

Run: `cargo test -p autologin-desktop --test login_command_contract`

Expected: FAIL because commands are absent.

- [ ] **Step 3: Implement commands**

Make `test_ai_connection` use an embedded credential-free fixture image. Return only `has_api_key` and `api_key_suffix`, never the key. Keep screenshots in a backend-owned expiring handle; the frontend may request a generated preview data URL only for the current confirmation task, and the handle becomes invalid on confirm, reject, cancel, or timeout.

```rust
#[tauri::command]
async fn get_ai_settings(state: State<'_, AppState>) -> Result<MaskedAiSettings, CommandError> {
    state.ai_settings.masked().await.map_err(Into::into)
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p autologin-desktop --test login_command_contract && cargo test --workspace`

Expected: PASS for authorized, stale-task, malformed-config, cancelled, and timeout cases.

- [ ] **Step 5: Commit**

```bash
git add app/src-tauri
git commit -m "feat: expose AI login commands"
```

## Task 8: Build AI settings, preview confirmation, and progress UI

**Files:**
- Create: `app/src/features/settings/AiSettings.tsx`
- Create: `app/src/features/login/LoginButton.tsx`
- Create: `app/src/features/login/TemplateConfirmation.tsx`
- Create: `app/src/features/login/LoginProgress.tsx`
- Create: `app/src/features/login/useLoginTask.ts`
- Modify: `app/src/features/applications/ApplicationAccounts.tsx`
- Modify: `app/src/locales/en.json`
- Modify: `app/src/locales/zh-CN.json`
- Test: `app/src/features/settings/AiSettings.test.tsx`
- Test: `app/src/features/login/LoginFlow.test.tsx`

**Interfaces:**
- Consumes: Task 7 commands and `login://state` events.
- Produces: provider setup, screenshot-upload consent, preview overlay confirmation, one-click login, cancellation, MFA notification state, and localized errors.

- [ ] **Step 1: Write settings tests**

Cover Gemini defaults, empty-key onboarding, masked saved key, replacing/deleting key, custom HTTPS Base URL, rejected public HTTP URL, explicit loopback HTTP confirmation, test connection, quota message, screenshot-upload consent, preview toggle, and English/Chinese rendering.

```tsx
it('shows Gemini defaults without exposing a stored key', async () => {
  invokeMock.mockResolvedValue({ provider: 'gemini', model: 'gemini-2.5-flash-lite', hasApiKey: true, apiKeySuffix: '1234' });
  render(<AiSettings />);
  expect(await screen.findByDisplayValue('gemini-2.5-flash-lite')).toBeVisible();
  expect(screen.queryByDisplayValue(/secret/)).toBeNull();
});
```

- [ ] **Step 2: Write login-flow tests**

Cover missing permission, known template one-click path, new template preview with three labeled rectangles, confirm/reject, cancel, already logged in, QR/verification, focus change, and terminal-state cleanup. Assert the DOM never contains the account password or full API key.

```tsx
it('requires confirmation for a newly analyzed template', async () => {
  render(<LoginFlow accountId="a1" />);
  emitLoginState({ state: 'awaitingTemplateConfirmation', previewHandle: 'p1', controls: threeControls });
  expect(await screen.findByRole('button', { name: 'Confirm controls' })).toBeVisible();
  expect(invokeMock).not.toHaveBeenCalledWith('confirm_template', expect.anything());
});
```

- [ ] **Step 3: Run tests and verify failure**

Run: `cd app && npm test -- --run src/features/settings/AiSettings.test.tsx src/features/login/LoginFlow.test.tsx`

Expected: FAIL because settings and login components do not exist.

- [ ] **Step 4: Implement UI and translations**

Use backend state as the source of truth. Draw normalized AI rectangles on a non-editable preview canvas, display provider/model and confidence, and require explicit confirmation for new templates. Disable navigation actions that could start a second task. Revoke preview object URLs and clear component state on every terminal event.

```tsx
return <PreviewCanvas imageUrl={previewUrl} controls={controls} readOnly aria-label={t('login.preview')} />;
```

- [ ] **Step 5: Run full automated verification**

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd app
npm test -- --run
npm run build
npx tauri build --debug
```

Expected: all commands pass.

- [ ] **Step 6: Perform macOS acceptance testing**

Use dedicated test accounts for WeChat, QQ, Steam, and two other application stacks. Verify permission onboarding, target-window-only preview, first-template confirmation, cached one-click login, window movement, scale/theme/language changes, focus theft, already-logged-in detection, QR/MFA stop, quota failure, network failure, cancellation, and absence of secrets/screenshots in disk files and Console logs.

- [ ] **Step 7: Commit**

```bash
git add app crates
git commit -m "feat: complete AI-guided desktop login"
```

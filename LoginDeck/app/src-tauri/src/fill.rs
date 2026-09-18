//! Single-owner desktop fill tasks. IPC contains progress and geometry, never secrets.
use autologin_core::{ApplicationAccountId, ApplicationsService};
use platform_runtime::login::{Cancellation, Field, ProbeError};
use serde::Serialize;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::{Duration, Instant};

#[derive(Clone, Serialize)]
pub struct FillSnapshot {
    pub id: u64,
    pub account_id: ApplicationAccountId,
    pub phase: String,
    pub preview: Option<[[f64; 4]; 3]>,
    pub error: Option<String>,
    pub username_attempted: bool,
    pub password_attempted: bool,
    pub logout_attempted: bool,
    pub logout_completed: bool,
}
struct Run {
    snapshot: FillSnapshot,
    cancel: Cancellation,
    sender: mpsc::Sender<()>,
    running: bool,
    last_seen: Instant,
}
#[derive(Clone, Default)]
pub struct FillManager {
    slot: Arc<Mutex<Option<Run>>>,
    next: Arc<AtomicU64>,
}
impl FillManager {
    fn reserve(
        &self,
        account_id: ApplicationAccountId,
    ) -> Result<(FillSnapshot, Cancellation, mpsc::Receiver<()>), String> {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        if slot.as_ref().is_some_and(|r| r.running) {
            return Err("busy".into());
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let snapshot = FillSnapshot {
            id,
            account_id,
            phase: "opening".into(),
            preview: None,
            error: None,
            username_attempted: false,
            password_attempted: false,
            logout_attempted: false,
            logout_completed: false,
        };
        let cancel = Cancellation::default();
        let (sender, receiver) = mpsc::channel();
        *slot = Some(Run {
            snapshot: snapshot.clone(),
            cancel: cancel.clone(),
            sender,
            running: true,
            last_seen: Instant::now(),
        });
        Ok((snapshot, cancel, receiver))
    }
    pub fn start(
        &self,
        account_id: ApplicationAccountId,
        service: ApplicationsService,
        attention: impl Fn() + Send + 'static,
    ) -> Result<FillSnapshot, String> {
        self.start_with_plan(account_id, service, None, attention)
    }
    pub fn start_with_plan(
        &self,
        account_id: ApplicationAccountId,
        service: ApplicationsService,
        plan: Option<crate::adapters::SwitchPlan>,
        attention: impl Fn() + Send + 'static,
    ) -> Result<FillSnapshot, String> {
        let (snapshot, cancel, receiver) = self.reserve(account_id)?;
        let manager = self.clone();
        let id = snapshot.id;
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                #[cfg(target_os = "macos")]
                if let Some(plan) = plan {
                    return manager.execute_switch(
                        id, account_id, service, &cancel, receiver, &attention, plan,
                    );
                }
                #[cfg(not(target_os = "macos"))]
                if plan.is_some() {
                    return Err("unsupported".into());
                }
                manager.execute(id, account_id, service, &cancel, receiver, &attention)
            }))
            .unwrap_or_else(|_| Err("internal".into()));
            let needs_attention = result.as_ref().err().is_some_and(|e| e != "cancelled");
            manager.change(id, |run| {
                run.running = false;
                match result {
                    Ok(phase) => run.snapshot.phase = phase.into(),
                    Err(error) => {
                        run.snapshot.phase = "stopped".into();
                        run.snapshot.error = Some(error);
                    }
                }
            });
            if needs_attention {
                attention();
            }
        });
        Ok(snapshot)
    }
    fn change(&self, id: u64, action: impl FnOnce(&mut Run)) {
        if let Some(run) = self
            .slot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
            .filter(|r| r.snapshot.id == id)
        {
            action(run);
        }
    }
    pub fn status(&self, id: u64) -> Result<FillSnapshot, String> {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        let run = slot
            .as_mut()
            .filter(|r| r.snapshot.id == id)
            .ok_or("expired")?;
        run.last_seen = Instant::now();
        Ok(run.snapshot.clone())
    }
    pub fn proceed(&self, id: u64) -> Result<(), String> {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        let run = slot
            .as_mut()
            .filter(|r| r.snapshot.id == id && r.running)
            .ok_or("expired")?;
        if run.cancel.check().is_err() {
            return Err("cancelled".into());
        }
        run.snapshot.phase = match run.snapshot.phase.as_str() {
            "confirm" | "switch_confirm" => "unlocking",
            "switch_ready" | "switch_challenge" => "switching",
            "prepare_login" => "checking",
            "ready" => "filling",
            "manual_sms" => "confirming_manual",
            _ => return Err("busy".into()),
        }
        .into();
        run.sender.send(()).map_err(|_| "expired".into())
    }
    pub fn cancel(&self, id: u64) {
        self.change(id, |run| {
            run.cancel.cancel();
            if run.running {
                run.snapshot.phase = "cancelling".into();
            }
        });
    }
    pub fn shutdown(&self) {
        if let Some(run) = self.slot.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            run.cancel.cancel();
        }
    }
    fn check(&self, id: u64, cancel: &Cancellation) -> Result<(), String> {
        cancel.check().map_err(reason)?;
        let slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        let run = slot
            .as_ref()
            .filter(|r| r.snapshot.id == id)
            .ok_or("expired")?;
        if run.last_seen.elapsed() > Duration::from_secs(15) {
            cancel.cancel();
            return Err("expired".into());
        }
        Ok(())
    }
    fn wait(
        &self,
        id: u64,
        cancel: &Cancellation,
        receiver: &mpsc::Receiver<()>,
    ) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            self.check(id, cancel)?;
            if Instant::now() > deadline {
                return Err("expired".into());
            }
            match receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(()) => return self.check(id, cancel),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err("cancelled".into()),
            }
        }
    }
    // Only a missing login page permits another read-only preparation attempt.
    // Credentials and input remain outside this loop.
    fn prepare_login<T>(
        &self,
        id: u64,
        cancel: &Cancellation,
        receiver: &mpsc::Receiver<()>,
        attention: &dyn Fn(),
        mut inspect: impl FnMut() -> Result<T, String>,
    ) -> Result<T, String> {
        loop {
            self.check(id, cancel)?;
            let result = inspect();
            self.check(id, cancel)?;
            match result {
                Ok(target) => return Ok(target),
                Err(error) if error == "login_page" => {
                    self.change(id, |run| {
                        run.snapshot.phase = "prepare_login".into();
                        run.snapshot.preview = None;
                    });
                    attention();
                    self.wait(id, cancel, receiver)?;
                }
                Err(error) => return Err(error),
            }
        }
    }
    #[cfg(target_os = "macos")]
    fn execute(
        &self,
        id: u64,
        account_id: ApplicationAccountId,
        service: ApplicationsService,
        cancel: &Cancellation,
        receiver: mpsc::Receiver<()>,
        attention: &dyn Fn(),
    ) -> Result<&'static str, String> {
        use platform_runtime::login::NativeFill;
        self.check(id, cancel)?;
        let method = tauri::async_runtime::block_on(service.account_login_method(account_id))
            .map_err(|e| core_reason(e.code()))?;
        if method != autologin_core::LoginMethod::Password {
            let account = tauri::async_runtime::block_on(service.prepare_manual_login(account_id))
                .map_err(|e| core_reason(e.code()))?;
            self.check(id, cancel)?;
            self.change(id, |run| run.snapshot.phase = "manual_sms".into());
            attention();
            self.wait(id, cancel, &receiver)?;
            tauri::async_runtime::block_on(service.check_fill_account(&account))
                .map_err(|e| core_reason(e.code()))?;
            self.check(id, cancel)?;
            return Ok("user_completed");
        }
        let mut expected = None;
        let (account, target, preview) =
            self.prepare_login(id, cancel, &receiver, attention, || {
                if let Some(account) = &expected {
                    tauri::async_runtime::block_on(service.check_fill_account(account))
                        .map_err(|e| core_reason(e.code()))?;
                }
                // Re-attest the application and bind its current window on every continuation.
                let (account, verified, adapter) =
                    tauri::async_runtime::block_on(service.prepare_fill(account_id))
                        .map_err(|e| core_reason(e.code()))?;
                if expected.as_ref().is_some_and(|old| old != &account) {
                    return Err("account_changed".into());
                }
                expected = Some(account.clone());
                self.check(id, cancel)?;
                let target = NativeFill::bind(&verified, &adapter).map_err(reason)?;
                let preview = target.preview().map_err(reason)?;
                Ok((account, target, preview))
            })?;
        self.change(id, |run| {
            run.snapshot.preview = Some(preview);
            run.snapshot.phase = "confirm".into();
        });
        attention();
        self.wait(id, cancel, &receiver)?;
        target.validate_preparation().map_err(reason)?;
        let password = tauri::async_runtime::block_on(service.reveal_for_fill(&account))
            .map_err(|e| core_reason(e.code()))?;
        self.check(id, cancel)?;
        target.validate_preparation().map_err(reason)?;
        // Keychain consent may have changed focus. Require a new explicit continuation,
        // so ending a system dialog never silently starts input in another application.
        self.change(id, |run| run.snapshot.phase = "ready".into());
        attention();
        self.wait(id, cancel, &receiver)?;
        tauri::async_runtime::block_on(service.check_fill_account(&account))
            .map_err(|e| core_reason(e.code()))?;
        target.activate(cancel).map_err(reason)?;
        self.check(id, cancel)?;
        target
            .fill(&account.username, &password, cancel, |field| {
                self.change(id, |run| match field {
                    Field::Username => run.snapshot.username_attempted = true,
                    Field::Password => run.snapshot.password_attempted = true,
                });
            })
            .map_err(reason)?;
        self.check(id, cancel).map(|()| "filled")
    }
    #[cfg(target_os = "macos")]
    #[allow(clippy::too_many_arguments)]
    fn execute_switch(
        &self,
        id: u64,
        account_id: ApplicationAccountId,
        service: ApplicationsService,
        cancel: &Cancellation,
        receiver: mpsc::Receiver<()>,
        attention: &dyn Fn(),
        plan: crate::adapters::SwitchPlan,
    ) -> Result<&'static str, String> {
        use autologin_core::{
            adapter::CredentialField,
            switcher::{Consent, Error, Progress, Runner, StateKind},
        };
        use platform_runtime::login::{activate_workflow_target, NativeWorkflow};
        self.check(id, cancel)?;
        let account = tauri::async_runtime::block_on(service.get_account(account_id))
            .map_err(|e| core_reason(e.code()))?;
        let app = tauri::async_runtime::block_on(service.get_application(account.application_id))
            .map_err(|e| core_reason(e.code()))?;
        if account.login_method != autologin_core::LoginMethod::Password
            || app.platform != plan.definition.platform
            || app.platform_application_id != plan.definition.application_id
        {
            return Err("adapter_mismatch".into());
        }
        self.change(id, |run| run.snapshot.phase = "switch_confirm".into());
        attention();
        self.wait(id, cancel, &receiver)?;
        let password = tauri::async_runtime::block_on(service.reveal_for_fill(&account))
            .map_err(|e| core_reason(e.code()))?;
        self.check(id, cancel)?;
        self.change(id, |run| run.snapshot.phase = "switch_ready".into());
        attention();
        self.wait(id, cancel, &receiver)?;
        let verified = tauri::async_runtime::block_on(service.prepare_switch(&account))
            .map_err(|e| core_reason(e.code()))?;
        self.check(id, cancel)?;
        activate_workflow_target(&verified, cancel).map_err(workflow_reason)?;
        let mut driver = NativeWorkflow::bind(
            &verified,
            &plan.definition,
            cancel.clone(),
            |field| {
                self.change(id, |run| match field {
                    CredentialField::Username => run.snapshot.username_attempted = true,
                    CredentialField::Password => run.snapshot.password_attempted = true,
                });
                Ok(match field {
                    CredentialField::Username => {
                        secrecy::SecretString::from(account.username.clone())
                    }
                    CredentialField::Password => password.clone(),
                })
            },
            || {
                self.check(id, cancel).map_err(|_| Error::Cancelled)?;
                tauri::async_runtime::block_on(service.check_fill_account(&account))
                    .map_err(|_| Error::TargetChanged)
            },
        )
        .map_err(workflow_reason)?;
        let snapshot = driver
            .observe_ready(Duration::from_secs(10))
            .map_err(workflow_reason)?;
        let detected = plan
            .definition
            .detect(&snapshot.nodes)
            .map_err(workflow_reason)?;
        let kind = plan.definition.states[&detected.state].kind;
        let flows: Vec<&str> = match kind {
            StateKind::LoggedIn => vec![&plan.logout, &plan.login],
            StateKind::LoginPage => vec![&plan.login],
            _ => return Err("login_page".into()),
        };
        for flow in flows {
            let logout = flow == plan.logout;
            let mut runner = Runner::new(
                plan.definition.clone(),
                flow,
                Consent {
                    logout: true,
                    submit: true,
                },
                Instant::now(),
            )
            .map_err(workflow_reason)?;
            loop {
                self.check(id, cancel)?;
                // A dispatched logout click may take effect even if AX reports failure.
                let progress = runner.tick(&mut driver, Instant::now());
                if logout && runner.attempted_actions > 0 {
                    self.change(id, |run| run.snapshot.logout_attempted = true);
                }
                match progress {
                    Progress::Running => std::thread::sleep(Duration::from_millis(100)),
                    Progress::WaitingForUser => {
                        self.change(id, |run| run.snapshot.phase = "switch_challenge".into());
                        attention();
                        self.wait(id, cancel, &receiver)?;
                        let current =
                            tauri::async_runtime::block_on(service.prepare_switch(&account))
                                .map_err(|e| core_reason(e.code()))?;
                        if &current != driver.verified_application() {
                            return Err("identity_changed".into());
                        }
                        activate_workflow_target(&current, cancel).map_err(workflow_reason)?;
                        runner.continue_challenge().map_err(workflow_reason)?;
                    }
                    Progress::Complete(_) => {
                        if logout {
                            self.change(id, |run| run.snapshot.logout_completed = true);
                        }
                        break;
                    }
                    Progress::Stopped(error) => return Err(workflow_reason(error)),
                }
            }
        }
        self.check(id, cancel)?;
        Ok("logged_in")
    }
    #[cfg(not(target_os = "macos"))]
    fn execute(
        &self,
        _: u64,
        _: ApplicationAccountId,
        _: ApplicationsService,
        _: &Cancellation,
        _: mpsc::Receiver<()>,
        _: &dyn Fn(),
    ) -> Result<&'static str, String> {
        Err("unsupported".into())
    }
}
fn core_reason(code: &str) -> String {
    match code {
        "credential.cancelled" | "credential.denied" => "credential_denied",
        "storage.conflict" | "storage.not_found" => "account_changed",
        "application.signature_changed" => "identity_changed",
        "application.unsupported_import" => "unsupported",
        "application.not_found" => "app_missing",
        _ => "unavailable",
    }
    .into()
}
fn reason(error: ProbeError) -> String {
    match error {
        ProbeError::PermissionRequired => "permission",
        ProbeError::AppUnavailable => "app_missing",
        ProbeError::AmbiguousTarget => "ambiguous",
        ProbeError::LoginFormUnavailable | ProbeError::UserActionRequired => "login_page",
        ProbeError::NotEmpty => "not_empty",
        ProbeError::NotFrontmost => "focus_changed",
        ProbeError::Cancelled => "cancelled",
        ProbeError::IdentityChanged => "identity_changed",
        ProbeError::Timeout => "timeout",
        ProbeError::Busy => "busy",
        ProbeError::TargetChanged | ProbeError::LayoutChanged | ProbeError::FocusNotConfirmed => {
            "target_changed"
        }
        _ => "unavailable",
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_page_waits_for_each_confirmation_then_rebinds() {
        let manager = FillManager::default();
        let (task, cancel, receiver) = manager.reserve(ApplicationAccountId::new()).unwrap();
        let attempts = std::cell::Cell::new(0);
        let confirmations = std::cell::Cell::new(0);
        let result = manager.prepare_login(
            task.id,
            &cancel,
            &receiver,
            &|| {
                assert_eq!(manager.status(task.id).unwrap().phase, "prepare_login");
                assert_eq!(attempts.get(), confirmations.get() + 1);
                assert!(receiver.try_recv().is_err());
                manager.proceed(task.id).unwrap();
                assert_eq!(manager.status(task.id).unwrap().phase, "checking");
                assert!(manager.proceed(task.id).is_err());
                confirmations.set(confirmations.get() + 1);
            },
            || {
                attempts.set(attempts.get() + 1);
                if attempts.get() < 3 {
                    Err("login_page".into())
                } else {
                    Ok(42)
                }
            },
        );
        assert_eq!(result, Ok(42));
        assert_eq!(confirmations.get(), 2);
        assert!(receiver.try_recv().is_err());
    }
    #[test]
    fn preparation_cancel_or_fatal_error_never_retries() {
        let manager = FillManager::default();
        let (task, cancel, receiver) = manager.reserve(ApplicationAccountId::new()).unwrap();
        let attempts = std::cell::Cell::new(0);
        let result: Result<(), String> = manager.prepare_login(
            task.id,
            &cancel,
            &receiver,
            &|| manager.cancel(task.id),
            || {
                attempts.set(attempts.get() + 1);
                Err("login_page".into())
            },
        );
        assert_eq!(result, Err("cancelled".into()));
        assert_eq!(attempts.get(), 1);
        for error in [
            "permission",
            "ambiguous",
            "identity_changed",
            "account_changed",
            "not_empty",
        ] {
            let manager = FillManager::default();
            let (task, cancel, receiver) = manager.reserve(ApplicationAccountId::new()).unwrap();
            let result: Result<(), String> = manager.prepare_login(
                task.id,
                &cancel,
                &receiver,
                &|| panic!("fatal errors must not offer continuation"),
                || Err(error.into()),
            );
            assert_eq!(result, Err(error.into()));
        }
    }
    #[test]
    fn manual_confirmation_never_enters_credential_filling_or_replays() {
        for (phase, next) in [
            ("manual_sms", "confirming_manual"),
            ("switch_confirm", "unlocking"),
            ("switch_ready", "switching"),
            ("switch_challenge", "switching"),
        ] {
            let manager = FillManager::default();
            let (task, _, receiver) = manager.reserve(ApplicationAccountId::new()).unwrap();
            manager.change(task.id, |run| run.snapshot.phase = phase.into());
            manager.proceed(task.id).unwrap();
            assert_eq!(manager.status(task.id).unwrap().phase, next);
            assert!(manager.proceed(task.id).is_err());
            assert!(receiver.try_recv().is_ok());
            assert!(receiver.try_recv().is_err());
            manager.cancel(task.id);
            assert!(manager.proceed(task.id).is_err());
        }
    }
    #[test]
    fn cancelled_worker_retains_ownership_until_it_finishes() {
        let manager = FillManager::default();
        let (task, cancel, _) = manager.reserve(ApplicationAccountId::new()).unwrap();
        manager.cancel(task.id);
        assert!(cancel.check().is_err());
        assert!(manager.reserve(ApplicationAccountId::new()).is_err());
        manager.change(task.id, |run| run.running = false);
        assert!(manager.reserve(ApplicationAccountId::new()).is_ok());
    }
    #[test]
    fn stale_or_duplicate_confirmation_cannot_replay_input() {
        let manager = FillManager::default();
        let (task, _, receiver) = manager.reserve(ApplicationAccountId::new()).unwrap();
        assert!(manager.proceed(task.id + 1).is_err());
        manager.change(task.id, |run| run.snapshot.phase = "confirm".into());
        manager.proceed(task.id).unwrap();
        assert!(manager.proceed(task.id).is_err());
        assert!(receiver.try_recv().is_ok());
        assert!(receiver.try_recv().is_err());
        manager.cancel(task.id);
        manager.change(task.id, |run| run.snapshot.phase = "ready".into());
        assert!(manager.proceed(task.id).is_err());
    }
    #[test]
    fn abandoned_dialog_expires_without_input() {
        let manager = FillManager::default();
        let (task, cancel, _) = manager.reserve(ApplicationAccountId::new()).unwrap();
        manager.change(task.id, |run| {
            run.last_seen = Instant::now() - Duration::from_secs(16)
        });
        assert!(manager.check(task.id, &cancel).is_err());
        assert!(cancel.check().is_err());
    }
}

fn workflow_reason(error: autologin_core::switcher::Error) -> String {
    use autologin_core::switcher::Error;
    match error {
        Error::Cancelled => "cancelled",
        Error::PermissionRequired => "permission",
        Error::Timeout => "timeout",
        Error::Ambiguous => "ambiguous",
        Error::UnknownState => "login_page",
        Error::TargetChanged | Error::WindowUnavailable | Error::ProcessExited => "target_changed",
        Error::FocusChanged => "focus_changed",
        Error::InvalidDefinition => "invalid_adapter",
        _ => "unavailable",
    }
    .into()
}

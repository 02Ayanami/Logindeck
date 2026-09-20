//! Native AX execution shared by validated application adapters.
//! Real credentials require NativeFill and a revalidated process attestation.
//! NativeLoginProbe exposes only fixed disposable fixture text.
//! No clipboard, screenshots, network requests, login presses or permission prompts.

use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "macos")]
mod native;
#[cfg(target_os = "macos")]
pub use native::{
    activate_workflow_target, capture_workflow_nodes, capture_workflow_pointer, inspect_adapter,
    NativeFill, NativeLoginProbe, NativeWorkflow, PointerCapture,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeError {
    PermissionRequired,
    AppUnavailable,
    AmbiguousTarget,
    LoginFormUnavailable,
    UserActionRequired,
    TargetChanged,
    FocusNotConfirmed,
    LayoutChanged,
    IdentityChanged,
    NotFrontmost,
    NotEmpty,
    Cancelled,
    Busy,
    Timeout,
    Accessibility(i32),
    UnsupportedAttribute,
}

/// No UI strings, field contents or credential material leave the native adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FormSummary {
    pub process_id: i32,
    pub username_writable: bool,
    pub password_writable: bool,
    pub submit_enabled: bool,
    pub frontmost: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Field {
    Username,
    Password,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbePhase {
    Ready,
    WritingUsername,
    WritingPassword,
    ClearingPassword,
    ClearingUsername,
    Complete,
    Stopped,
}

/// Records attempted mutations too: an AX timeout does not mean nothing happened.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProbeReport {
    pub phase: ProbePhase,
    pub last_operation: ProbePhase,
    pub username_may_remain: bool,
    pub password_may_remain: bool,
    pub error: Option<ProbeError>,
}

#[derive(Default, Clone)]
pub struct Cancellation(std::sync::Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn check(&self) -> Result<(), ProbeError> {
        if self.0.load(Ordering::SeqCst) {
            Err(ProbeError::Cancelled)
        } else {
            Ok(())
        }
    }
}

static ACTIVE: AtomicBool = AtomicBool::new(false);
pub(super) struct Lease;
impl Lease {
    fn acquire() -> Result<Self, ProbeError> {
        ACTIVE
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map(|_| Self)
            .map_err(|_| ProbeError::Busy)
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::SeqCst);
    }
}

trait FixtureTarget {
    fn guard(&self) -> Result<(), ProbeError>;
    fn ensure_empty(&self) -> Result<(), ProbeError>;
    /// Must recheck focus/target after focusing the field and before AXSetValue.
    fn write(&self, field: Field, text: &str, cancel: &Cancellation) -> Result<(), ProbeError>;
    fn verify(&self, field: Field, empty: bool, cancel: &Cancellation) -> Result<(), ProbeError>;
}

// AX focus changes can be acknowledged before the focused element is updated.
// Only the old element is tolerated while waiting; any third element aborts.
fn focus_transition<T: PartialEq>(
    current: &T,
    previous: &T,
    target: &T,
) -> Result<bool, ProbeError> {
    if current == target {
        Ok(true)
    } else if current == previous {
        Ok(false)
    } else {
        Err(ProbeError::FocusNotConfirmed)
    }
}

fn exercise(target: &impl FixtureTarget, cancel: &Cancellation) -> ProbeReport {
    let mut report = ProbeReport {
        phase: ProbePhase::Ready,
        last_operation: ProbePhase::Ready,
        username_may_remain: false,
        password_may_remain: false,
        error: None,
    };
    let result = (|| {
        cancel.check()?;
        target.guard()?;
        target.ensure_empty()?;
        for (phase, field, text) in [
            (ProbePhase::WritingUsername, Field::Username, "0000000000"),
            (
                ProbePhase::WritingPassword,
                Field::Password,
                "LoginDeckTestOnly!",
            ),
            (ProbePhase::ClearingPassword, Field::Password, ""),
            (ProbePhase::ClearingUsername, Field::Username, ""),
        ] {
            cancel.check()?;
            target.guard()?;
            report.phase = phase;
            report.last_operation = phase;
            let pending = match field {
                Field::Username => &mut report.username_may_remain,
                Field::Password => &mut report.password_may_remain,
            };
            *pending = true;
            target.write(field, text, cancel)?;
            target.verify(field, text.is_empty(), cancel)?;
            if text.is_empty() {
                *pending = false;
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => report.phase = ProbePhase::Complete,
        Err(error) => {
            report.phase = ProbePhase::Stopped;
            report.error = Some(error);
        }
    }
    // Never blindly clean up after a focus change/cancel/error; the user handles remnants.
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[test]
    fn asynchronous_focus_accepts_only_the_expected_transition() {
        assert_eq!(
            focus_transition(&"username", &"username", &"password"),
            Ok(false)
        );
        assert_eq!(
            focus_transition(&"password", &"username", &"password"),
            Ok(true)
        );
        assert_eq!(
            focus_transition(&"other", &"username", &"password"),
            Err(ProbeError::FocusNotConfirmed)
        );
    }

    struct Fake<'a> {
        writes: RefCell<Vec<(Field, String)>>,
        after: Option<ProbeError>,
        checks: Cell<usize>,
        nonempty: bool,
        cancel_after_write: Option<&'a Cancellation>,
        write_error: bool,
    }
    impl FixtureTarget for Fake<'_> {
        fn guard(&self) -> Result<(), ProbeError> {
            self.checks.set(self.checks.get() + 1);
            if !self.writes.borrow().is_empty() {
                if let Some(error) = self.after {
                    return Err(error);
                }
            }
            Ok(())
        }
        fn ensure_empty(&self) -> Result<(), ProbeError> {
            if self.nonempty {
                Err(ProbeError::NotEmpty)
            } else {
                Ok(())
            }
        }
        fn write(&self, field: Field, text: &str, _: &Cancellation) -> Result<(), ProbeError> {
            self.writes.borrow_mut().push((field, text.into()));
            if let Some(cancel) = self.cancel_after_write {
                cancel.cancel();
            }
            if self.write_error {
                Err(ProbeError::Timeout)
            } else {
                Ok(())
            }
        }
        fn verify(&self, _: Field, _: bool, cancel: &Cancellation) -> Result<(), ProbeError> {
            cancel.check()?;
            self.guard()
        }
    }
    fn fake() -> Fake<'static> {
        Fake {
            writes: RefCell::new(vec![]),
            after: None,
            checks: Cell::new(0),
            nonempty: false,
            cancel_after_write: None,
            write_error: false,
        }
    }
    #[test]
    fn completed_probe_clears_both_fields_and_never_submits() {
        let target = fake();
        let report = exercise(&target, &Cancellation::default());
        assert_eq!(report.phase, ProbePhase::Complete);
        assert!(!report.username_may_remain && !report.password_may_remain);
        assert_eq!(
            &target.writes.borrow()[2..],
            &[(Field::Password, "".into()), (Field::Username, "".into())]
        );
    }
    #[test]
    fn changed_window_or_focus_prevents_password_and_cleanup() {
        for error in [ProbeError::TargetChanged, ProbeError::NotFrontmost] {
            let mut target = fake();
            target.after = Some(error);
            let report = exercise(&target, &Cancellation::default());
            assert_eq!(report.error, Some(error));
            assert_eq!(target.writes.borrow().len(), 1);
            assert!(report.username_may_remain);
            assert!(!report.password_may_remain);
        }
    }
    #[test]
    fn cancelled_tasks_never_resume_or_clean_up_automatically() {
        let cancel = Cancellation::default();
        let mut target = fake();
        target.cancel_after_write = Some(&cancel);
        assert_eq!(
            exercise(&target, &cancel).error,
            Some(ProbeError::Cancelled)
        );
        assert_eq!(target.writes.borrow().len(), 1);
        assert_eq!(
            exercise(&target, &cancel).error,
            Some(ProbeError::Cancelled)
        );
        assert_eq!(target.writes.borrow().len(), 1);
    }
    #[test]
    fn existing_data_is_never_overwritten() {
        let mut target = fake();
        target.nonempty = true;
        assert_eq!(
            exercise(&target, &Cancellation::default()).error,
            Some(ProbeError::NotEmpty)
        );
        assert!(target.writes.borrow().is_empty());
    }
    #[test]
    fn timeout_after_attempt_does_not_retry_or_claim_clean() {
        let mut target = fake();
        target.write_error = true;
        let report = exercise(&target, &Cancellation::default());
        assert_eq!(report.error, Some(ProbeError::Timeout));
        assert!(report.username_may_remain);
        assert_eq!(target.writes.borrow().len(), 1);
    }
    #[test]
    fn only_one_probe_can_own_the_executor() {
        let lease = Lease::acquire().unwrap();
        assert!(matches!(Lease::acquire(), Err(ProbeError::Busy)));
        drop(lease);
        assert!(Lease::acquire().is_ok());
    }
}

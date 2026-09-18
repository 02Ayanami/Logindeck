//! Platform-neutral v2 workflow runtime. App-specific paths live in data.
//! A platform driver owns attestation, native handles and backend credentials.
pub mod builder;

use crate::adapter::{CredentialField, Node, Role, Selector};
use crate::Platform;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    schema_version: u32,
    pub id: String,
    pub platform: Platform,
    pub application_id: String,
    pub states: BTreeMap<String, State>,
    pub flows: BTreeMap<String, Flow>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub kind: StateKind,
    pub controls: BTreeMap<String, Selector>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub absent: Vec<Selector>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StateKind {
    LoggedIn,
    LoginPage,
    Intermediate,
    Challenge,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Flow {
    pub kind: FlowKind,
    pub timeout_ms: u64,
    pub steps: Vec<Step>,
    pub outcome: Outcome,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FlowKind {
    Logout,
    Login,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    LoggedOut,
    Filled,
    Submitted,
    LoggedInObserved,
}
/// The recorded execution method; never an automatic fallback chain.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClickMethod {
    #[default]
    Accessibility,
    Pointer,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    Click {
        state: String,
        target: String,
        next_states: Vec<String>,
        #[serde(default)]
        method: ClickMethod,
    },
    Fill {
        state: String,
        target: String,
        field: CredentialField,
    },
    Clear {
        state: String,
        target: String,
        field: CredentialField,
    },
    Check {
        state: String,
        target: String,
    },
    Submit {
        state: String,
        target: String,
        next_states: Vec<String>,
    },
    WaitState {
        state: String,
    },
    Challenge {
        state: String,
        resume_state: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidDefinition,
    UnknownState,
    Ambiguous,
    InvalidTree,
    ConsentRequired,
    PermissionRequired,
    Cancelled,
    Timeout,
    TargetChanged,
    ProcessExited,
    FocusChanged,
    WindowUnavailable,
    TargetObscured,
    InputBusy,
    Driver,
    Stopped,
    NotWaiting,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Progress {
    Running,
    WaitingForUser,
    Complete(Outcome),
    Stopped(Error),
}

/// No executable paths, credentials, scripts or arbitrary values may be embedded.
impl Definition {
    pub fn parse(source: &str) -> Result<Self, Error> {
        if source.len() > 64 * 1024 {
            return Err(Error::InvalidDefinition);
        }
        let mut value: serde_json::Value =
            serde_json::from_str(source).map_err(|_| Error::InvalidDefinition)?;
        migrate_v2(&mut value)?;
        let definition: Self =
            serde_json::from_value(value).map_err(|_| Error::InvalidDefinition)?;
        definition.validate()?;
        Ok(definition)
    }
    pub fn to_json(&self) -> Result<String, Error> {
        self.validate()?;
        let json = serde_json::to_string_pretty(self).map_err(|_| Error::InvalidDefinition)?;
        if json.len() > 64 * 1024 {
            return Err(Error::InvalidDefinition);
        }
        Ok(json)
    }
    fn validate(&self) -> Result<(), Error> {
        let invalid = || Error::InvalidDefinition;
        if self.schema_version != 3
            || !name(&self.id)
            || !name(&self.application_id)
            || self.states.is_empty()
            || self.states.len() > 32
            || self.flows.is_empty()
            || self.flows.len() > 8
        {
            return Err(invalid());
        }
        for (id, state) in &self.states {
            if !name(id)
                || state.controls.is_empty()
                || state.controls.len() > 16
                || state.absent.len() > 16
                || state.absent.iter().any(|s| !s.valid())
                || state.controls.iter().any(|(id, s)| !name(id) || !s.valid())
            {
                return Err(invalid());
            }
        }
        for (id, flow) in &self.flows {
            if !name(id)
                || !(100..=300_000).contains(&flow.timeout_ms)
                || flow.steps.is_empty()
                || flow.steps.len() > 64
            {
                return Err(invalid());
            }
            // Steps must form a coherent path; Builder can reject broken recordings
            // before a workflow ever receives native execution authority.
            for pair in flow.steps.windows(2) {
                let reaches_next = match &pair[0] {
                    Step::Click { next_states, .. } | Step::Submit { next_states, .. } => {
                        next_states.iter().any(|state| state == pair[1].state())
                    }
                    Step::Challenge { resume_state, .. } => resume_state == pair[1].state(),
                    step => step.state() == pair[1].state(),
                };
                if !reaches_next {
                    return Err(invalid());
                }
            }
            if flow.kind == FlowKind::Logout
                && self
                    .states
                    .get(flow.steps[0].state())
                    .is_none_or(|s| s.kind != StateKind::LoggedIn)
            {
                return Err(invalid());
            }
            let mut fills = 0;
            let mut submits = 0;
            for step in &flow.steps {
                let state = self.states.get(step.state()).ok_or_else(invalid)?;
                match step {
                    Step::Click {
                        target,
                        next_states,
                        ..
                    }
                    | Step::Submit {
                        target,
                        next_states,
                        ..
                    } => {
                        if state
                            .controls
                            .get(target)
                            .is_none_or(|s| !matches!(s.role(), Role::Button | Role::MenuItem))
                            || next_states.is_empty()
                            || next_states.len() > 8
                            || next_states
                                .iter()
                                .any(|next| next == step.state() || !self.states.contains_key(next))
                            || next_states
                                .iter()
                                .enumerate()
                                .any(|(i, next)| next_states[..i].contains(next))
                        {
                            return Err(invalid());
                        }
                        if matches!(step, Step::Submit { .. }) {
                            submits += 1;
                        }
                    }
                    Step::Fill { target, field, .. } | Step::Clear { target, field, .. } => {
                        let expected = match field {
                            CredentialField::Username => Role::TextField,
                            CredentialField::Password => Role::SecureTextField,
                        };
                        if state.kind != StateKind::LoginPage
                            || state
                                .controls
                                .get(target)
                                .is_none_or(|s| s.role() != expected)
                        {
                            return Err(invalid());
                        }
                        if matches!(step, Step::Fill { .. }) {
                            fills += 1;
                        }
                    }
                    Step::Check { target, .. } => {
                        if state.kind != StateKind::LoginPage
                            || state
                                .controls
                                .get(target)
                                .is_none_or(|selector| selector.role() != Role::Checkbox)
                        {
                            return Err(invalid());
                        }
                    }
                    Step::Challenge { resume_state, .. } => {
                        if state.kind != StateKind::Challenge
                            || !self.states.contains_key(resume_state)
                        {
                            return Err(invalid());
                        }
                    }
                    Step::WaitState { .. } => {}
                }
            }
            let last = flow.steps.last().ok_or_else(invalid)?;
            let final_kind = self.states[last.state()].kind;
            match flow.outcome {
                Outcome::LoggedOut
                    if flow.kind == FlowKind::Logout
                        && fills == 0
                        && submits == 0
                        && matches!(last, Step::WaitState { .. })
                        && final_kind == StateKind::LoginPage => {}
                Outcome::Filled
                    if flow.kind == FlowKind::Login
                        && fills > 0
                        && submits == 0
                        && matches!(last, Step::Fill { .. }) => {}
                Outcome::Submitted
                    if flow.kind == FlowKind::Login
                        && submits == 1
                        && matches!(last, Step::Submit { .. }) => {}
                Outcome::LoggedInObserved
                    if flow.kind == FlowKind::Login
                        && submits == 1
                        && matches!(last, Step::WaitState { .. })
                        && final_kind == StateKind::LoggedIn => {}
                _ => return Err(invalid()),
            }
        }
        Ok(())
    }
    pub fn detect(&self, nodes: &[Node]) -> Result<Detected, Error> {
        if nodes.len() > 512
            || nodes
                .iter()
                .enumerate()
                .any(|(i, n)| n.parent.is_some_and(|p| p >= i))
        {
            return Err(Error::InvalidTree);
        }
        let mut found = None;
        for (id, state) in &self.states {
            if state.absent.iter().any(|selector| {
                nodes
                    .iter()
                    .enumerate()
                    .any(|(i, node)| selector.matches(i, node, nodes))
            }) {
                continue;
            }
            let mut controls = BTreeMap::new();
            let mut ambiguous = false;
            let mut missing = false;
            for (id, selector) in &state.controls {
                let matches: Vec<_> = nodes
                    .iter()
                    .enumerate()
                    .filter(|(i, n)| selector.matches(*i, n, nodes))
                    .map(|(i, _)| i)
                    .collect();
                missing |= matches.is_empty();
                ambiguous |= matches.len() > 1;
                if let Some(index) = matches.first() {
                    controls.insert(id.clone(), *index);
                }
            }
            if missing {
                continue;
            }
            if ambiguous || found.is_some() {
                return Err(Error::Ambiguous);
            }
            found = Some(Detected {
                state: id.clone(),
                controls,
            });
        }
        found.ok_or(Error::UnknownState)
    }
}
fn migrate_v2(value: &mut serde_json::Value) -> Result<(), Error> {
    let version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or(Error::InvalidDefinition)?;
    if version == 3 {
        return Ok(());
    }
    if version != 2 {
        return Err(Error::InvalidDefinition);
    }
    let flows = value
        .get_mut("flows")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or(Error::InvalidDefinition)?;
    for flow in flows.values_mut() {
        let steps = flow
            .get_mut("steps")
            .and_then(serde_json::Value::as_array_mut)
            .ok_or(Error::InvalidDefinition)?;
        for step in steps {
            let Some(object) = step.as_object_mut() else {
                return Err(Error::InvalidDefinition);
            };
            object.remove("expect_restart");
            if let Some(next) = object.remove("next_state") {
                object.insert("next_states".into(), serde_json::Value::Array(vec![next]));
            }
        }
    }
    value["schema_version"] = serde_json::Value::from(3);
    Ok(())
}
fn name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
impl Step {
    fn state(&self) -> &str {
        match self {
            Self::Click { state, .. }
            | Self::Fill { state, .. }
            | Self::Clear { state, .. }
            | Self::Check { state, .. }
            | Self::Submit { state, .. }
            | Self::WaitState { state }
            | Self::Challenge { state, .. } => state,
        }
    }
}
pub struct Detected {
    pub state: String,
    pub controls: BTreeMap<String, usize>,
}
/// Driver-owned generation token ties indices to one native observation.
/// The driver must reject stale tokens and guard process identity, window, focus,
/// cancellation and account revision immediately before every mutation.
pub struct Snapshot {
    pub token: u64,
    pub nodes: Vec<Node>,
}
pub trait Driver {
    fn guard(&mut self) -> Result<(), Error>;
    fn observe(&mut self) -> Result<Snapshot, Error>;
    fn click(&mut self, snapshot: &Snapshot, node: usize) -> Result<(), Error>;
    /// Dispatch once and tolerate a renderer, window or verified process replacement.
    fn click_transitioning(
        &mut self,
        snapshot: &Snapshot,
        node: usize,
        _timeout: Duration,
    ) -> Result<(), Error> {
        self.click(snapshot, node)
    }
    fn pointer_click(&mut self, _snapshot: &Snapshot, _node: usize) -> Result<(), Error> {
        Err(Error::Driver)
    }
    fn pointer_click_transitioning(
        &mut self,
        snapshot: &Snapshot,
        node: usize,
        _timeout: Duration,
    ) -> Result<(), Error> {
        self.pointer_click(snapshot, node)
    }
    fn click_configured(
        &mut self,
        snapshot: &Snapshot,
        node: usize,
        method: ClickMethod,
        timeout: Duration,
    ) -> Result<(), Error> {
        match method {
            ClickMethod::Accessibility => self.click_transitioning(snapshot, node, timeout),
            ClickMethod::Pointer => self.pointer_click_transitioning(snapshot, node, timeout),
        }
    }
    /// The driver reads the chosen backend account only; no secrets reach config/runtime.
    fn fill(
        &mut self,
        snapshot: &Snapshot,
        node: usize,
        field: CredentialField,
    ) -> Result<(), Error>;
    /// Explicit destructive field mutation. Unsupported drivers reject it.
    fn clear(
        &mut self,
        _snapshot: &Snapshot,
        _node: usize,
        _field: CredentialField,
    ) -> Result<(), Error> {
        Err(Error::Driver)
    }
    /// Idempotently select a checkbox. Unsupported drivers reject before input.
    fn check(&mut self, _snapshot: &Snapshot, _node: usize) -> Result<(), Error> {
        Err(Error::Driver)
    }
}
/// Supplied by the task UI, never deserialized from an adapter.
#[derive(Default)]
pub struct Consent {
    pub logout: bool,
    pub submit: bool,
}
pub struct Runner {
    definition: Definition,
    flow: Flow,
    step: usize,
    pending_states: Option<Vec<String>>,
    progress: Progress,
    deadline: Instant,
    last_tick: Instant,
    /// Attempt count includes errors: a timed-out click may already have happened.
    pub attempted_actions: usize,
}
impl Runner {
    pub fn new(
        definition: Definition,
        flow: &str,
        consent: Consent,
        now: Instant,
    ) -> Result<Self, Error> {
        definition.validate()?;
        let flow = definition
            .flows
            .get(flow)
            .ok_or(Error::InvalidDefinition)?
            .clone();
        if (flow.kind == FlowKind::Logout && !consent.logout)
            || (flow.steps.iter().any(|s| matches!(s, Step::Submit { .. })) && !consent.submit)
        {
            return Err(Error::ConsentRequired);
        }
        Ok(Self {
            deadline: now + Duration::from_millis(flow.timeout_ms),
            last_tick: now,
            definition,
            flow,
            step: 0,
            pending_states: None,
            progress: Progress::Running,
            attempted_actions: 0,
        })
    }
    pub fn progress(&self) -> &Progress {
        &self.progress
    }
    pub fn cancel(&mut self) {
        if !matches!(self.progress, Progress::Complete(_) | Progress::Stopped(_)) {
            self.progress = Progress::Stopped(Error::Cancelled);
        }
    }
    pub fn continue_challenge(&mut self) -> Result<(), Error> {
        if self.progress != Progress::WaitingForUser {
            return Err(Error::NotWaiting);
        }
        let Step::Challenge { resume_state, .. } = &self.flow.steps[self.step] else {
            return Err(Error::NotWaiting);
        };
        self.pending_states = Some(vec![resume_state.clone()]);
        self.step += 1;
        self.progress = Progress::Running;
        Ok(())
    }
    /// Caller schedules polling; each tick executes at most one mutation. Only
    /// observations are retried. An attempted mutation is never automatically replayed.
    pub fn tick(&mut self, driver: &mut impl Driver, now: Instant) -> Progress {
        if matches!(self.progress, Progress::Complete(_) | Progress::Stopped(_)) {
            return self.progress.clone();
        }
        let result = if now < self.last_tick {
            Err(Error::TargetChanged)
        } else if now >= self.deadline {
            Err(Error::Timeout)
        } else {
            self.advance(driver)
        };
        self.last_tick = now;
        if let Err(error) = result {
            self.progress = Progress::Stopped(error);
        }
        self.progress.clone()
    }
    fn advance(&mut self, driver: &mut impl Driver) -> Result<(), Error> {
        driver.guard()?;
        if self.progress == Progress::WaitingForUser {
            return Ok(());
        }
        let snapshot = match driver.observe() {
            Ok(snapshot) => snapshot,
            // A dispatched action can replace a window. Wait only for an
            // expected transition; never resend the previous mutation.
            Err(Error::WindowUnavailable | Error::ProcessExited)
                if self.pending_states.is_some() =>
            {
                return Ok(())
            }
            Err(error) => return Err(error),
        };
        driver.guard()?;
        let detected = match self.definition.detect(&snapshot.nodes) {
            Ok(d) => d,
            Err(Error::UnknownState) => return Ok(()),
            Err(e) => return Err(e),
        };
        if let Some(expected) = &self.pending_states {
            if !expected.contains(&detected.state) {
                return Ok(());
            }
            self.pending_states = None;
            if self.step < self.flow.steps.len()
                && self.flow.steps[self.step].state() != detected.state
            {
                let Some(next) = self.flow.steps[self.step..]
                    .iter()
                    .position(|step| step.state() == detected.state)
                else {
                    return Err(Error::TargetChanged);
                };
                self.step += next;
            }
        } else if self.step < self.flow.steps.len()
            && self.flow.steps[self.step].state() != detected.state
        {
            let Some(next) = self.flow.steps[self.step..]
                .iter()
                .position(|step| step.state() == detected.state)
            else {
                return Ok(());
            };
            self.step += next;
        }
        if self.step == self.flow.steps.len() {
            self.progress = Progress::Complete(self.flow.outcome);
            return Ok(());
        }
        let step = &self.flow.steps[self.step];
        if detected.state != step.state() {
            return Ok(());
        }
        match step {
            Step::Click {
                target,
                next_states,
                method,
                ..
            } => {
                driver.guard()?;
                self.attempted_actions += 1;
                driver.click_configured(
                    &snapshot,
                    detected.controls[target],
                    *method,
                    self.deadline.saturating_duration_since(Instant::now()),
                )?;
                self.pending_states = Some(next_states.clone());
            }
            Step::Submit {
                target,
                next_states,
                ..
            } => {
                driver.guard()?;
                self.attempted_actions += 1;
                driver.click_configured(
                    &snapshot,
                    detected.controls[target],
                    ClickMethod::Accessibility,
                    self.deadline.saturating_duration_since(Instant::now()),
                )?;
                self.pending_states = Some(next_states.clone());
            }
            Step::Fill { target, field, .. } => {
                driver.guard()?;
                self.attempted_actions += 1;
                driver.fill(&snapshot, detected.controls[target], *field)?;
            }
            Step::Clear { target, field, .. } => {
                driver.guard()?;
                self.attempted_actions += 1;
                driver.clear(&snapshot, detected.controls[target], *field)?;
            }
            Step::Check { target, .. } => {
                driver.guard()?;
                self.attempted_actions += 1;
                driver.check(&snapshot, detected.controls[target])?;
            }
            Step::Challenge { .. } => {
                self.progress = Progress::WaitingForUser;
                return Ok(());
            }
            Step::WaitState { .. } => {}
        }
        driver.guard()?;
        self.step += 1;
        if self.step == self.flow.steps.len() && self.pending_states.is_none() {
            self.progress = Progress::Complete(self.flow.outcome);
        }
        Ok(())
    }
}

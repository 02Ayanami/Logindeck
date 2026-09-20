mod workflow;
use super::*;
use core_foundation::{
    array::{CFArrayGetCount, CFArrayGetTypeID, CFArrayGetValueAtIndex},
    base::{CFGetTypeID, CFType, CFTypeRef, TCFType},
    boolean::CFBoolean,
    number::CFNumber,
    string::{CFString, CFStringGetLength, CFStringRef},
};
use objc2::rc::Retained;
use objc2_app_kit::NSRunningApplication;
use objc2_foundation::NSString;
use std::{
    ffi::c_void,
    ptr,
    time::{Duration, Instant},
};
pub use workflow::{
    activate_workflow_target, capture_workflow_nodes, capture_workflow_pointer, NativeWorkflow,
    PointerCapture,
};

type AxRef = *const c_void;
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> AxRef;
    fn GetFrontProcess(process: *mut ProcessSerialNumber) -> i16;
    fn GetProcessPID(process: *const ProcessSerialNumber, pid: *mut i32) -> i32;
    fn AXUIElementGetTypeID() -> usize;
    fn AXUIElementCopyAttributeValue(
        element: AxRef,
        name: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementSetAttributeValue(element: AxRef, name: CFStringRef, value: CFTypeRef) -> i32;
    fn AXUIElementIsAttributeSettable(element: AxRef, name: CFStringRef, value: *mut u8) -> i32;
    fn AXUIElementSetMessagingTimeout(element: AxRef, seconds: f32) -> i32;
}

#[derive(Clone, PartialEq)]
struct Ax(CFType);
impl Ax {
    fn raw(&self) -> AxRef {
        self.0.as_CFTypeRef()
    }
    fn from_value(value: CFType) -> Result<Self, ProbeError> {
        if value.type_of() != unsafe { AXUIElementGetTypeID() } {
            return Err(ProbeError::UnsupportedAttribute);
        }
        Ok(Self(value))
    }
    fn optional(&self, name: &str) -> Result<Option<CFType>, ProbeError> {
        let mut value = ptr::null();
        let status = unsafe {
            AXUIElementCopyAttributeValue(
                self.raw(),
                CFString::new(name).as_concrete_TypeRef(),
                &mut value,
            )
        };
        // AttributeUnsupported and NoValue are expected for optional metadata only.
        if status == -25205 || status == -25212 {
            return Ok(None);
        }
        check(status)?;
        if value.is_null() {
            return Err(ProbeError::UnsupportedAttribute);
        }
        Ok(Some(unsafe { CFType::wrap_under_create_rule(value) }))
    }
    fn value(&self, name: &str) -> Result<CFType, ProbeError> {
        self.optional(name)?.ok_or(ProbeError::UnsupportedAttribute)
    }
    fn text(&self, name: &str) -> Result<Option<String>, ProbeError> {
        self.optional(name)?
            .map(|value| {
                value
                    .downcast::<CFString>()
                    .map(|s| s.to_string())
                    .ok_or(ProbeError::UnsupportedAttribute)
            })
            .transpose()
    }
    fn flag(&self, name: &str) -> Result<bool, ProbeError> {
        self.value(name)?
            .downcast::<CFBoolean>()
            .map(bool::from)
            .ok_or(ProbeError::UnsupportedAttribute)
    }
    fn checked(&self) -> Result<bool, ProbeError> {
        let value = self.value("AXValue")?;
        if let Some(value) = value.clone().downcast::<CFBoolean>() {
            return Ok(bool::from(value));
        }
        value
            .downcast::<CFNumber>()
            .and_then(|number| number.to_i32())
            .map(|number| number != 0)
            .ok_or(ProbeError::UnsupportedAttribute)
    }
    fn children(&self, name: &str) -> Result<Vec<Ax>, ProbeError> {
        let Some(value) = self.optional(name)? else {
            return Ok(vec![]);
        };
        unsafe {
            if CFGetTypeID(value.as_CFTypeRef()) != CFArrayGetTypeID() {
                return Err(ProbeError::UnsupportedAttribute);
            }
            let array = value.as_CFTypeRef().cast();
            let count = CFArrayGetCount(array);
            if count > 512 {
                return Err(ProbeError::AmbiguousTarget);
            }
            (0..count)
                .map(|i| {
                    let child = CFArrayGetValueAtIndex(array, i);
                    if child.is_null() {
                        return Err(ProbeError::UnsupportedAttribute);
                    }
                    Ax::from_value(CFType::wrap_under_get_rule(child))
                })
                .collect()
        }
    }
    fn writable(&self) -> Result<bool, ProbeError> {
        let mut value = 0;
        check(unsafe {
            AXUIElementIsAttributeSettable(
                self.raw(),
                CFString::new("AXValue").as_concrete_TypeRef(),
                &mut value,
            )
        })?;
        Ok(value != 0)
    }
    fn set(&self, name: &str, value: &impl TCFType) -> Result<(), ProbeError> {
        check(unsafe {
            AXUIElementSetAttributeValue(
                self.raw(),
                CFString::new(name).as_concrete_TypeRef(),
                value.as_CFTypeRef(),
            )
        })
    }
    fn empty(&self) -> Result<bool, ProbeError> {
        // Do not stringify or log the secure field; no value means unknown, not empty.
        let value = self
            .value("AXValue")?
            .downcast::<CFString>()
            .ok_or(ProbeError::UnsupportedAttribute)?;
        Ok(unsafe { CFStringGetLength(value.as_concrete_TypeRef()) } == 0)
    }
}
fn check(status: i32) -> Result<(), ProbeError> {
    match status {
        0 => Ok(()),
        -25211 => Err(ProbeError::PermissionRequired),
        other => Err(ProbeError::Accessibility(other)),
    }
}

#[derive(Clone, PartialEq)]
struct Form {
    window: Ax,
    username: Ax,
    password: Ax,
    submit: Ax,
    // Changes in window or control geometry require a fresh explicit run.
    geometry: Vec<CFType>,
    state_id: String,
    actions: Vec<autologin_core::adapter::Action>,
}
fn unique(mut items: Vec<Ax>) -> Result<Ax, ProbeError> {
    match items.len() {
        1 => Ok(items.remove(0)),
        0 => Err(ProbeError::LoginFormUnavailable),
        _ => Err(ProbeError::AmbiguousTarget),
    }
}
fn recognize(
    app: &Ax,
    adapter: &autologin_core::adapter::AppDefinition,
) -> Result<Form, ProbeError> {
    use autologin_core::adapter::{AdapterError, Control, Node, Role, StateKind};
    let window = unique(app.children("AXWindows")?)?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut stack = vec![(window.clone(), 0, None)];
    let mut elements = Vec::new();
    let mut nodes = Vec::new();
    while let Some((node, depth, parent)) = stack.pop() {
        if Instant::now() > deadline {
            return Err(ProbeError::Timeout);
        }
        if elements.len() >= 512 || depth > 24 {
            return Err(ProbeError::AmbiguousTarget);
        }
        let mut names = Vec::new();
        for name in ["AXTitle", "AXDescription", "AXPlaceholderValue"] {
            if let Some(value) = node.text(name)? {
                names.push(value);
            }
        }
        let index = elements.len();
        let role = match node.text("AXRole")?.as_deref() {
            Some("AXTextField")
                if node.text("AXSubrole")?.as_deref() == Some("AXSecureTextField") =>
            {
                Some(Role::SecureTextField)
            }
            Some("AXTextField") => Some(Role::TextField),
            Some("AXButton") => Some(Role::Button),
            Some("AXCheckBox") => Some(Role::Checkbox),
            Some("AXMenuItem") => Some(Role::MenuItem),
            Some("AXMenu") => Some(Role::Menu),
            Some("AXStaticText") => Some(Role::StaticText),
            Some("AXGroup") => Some(Role::Group),
            Some("AXWebArea") => Some(Role::WebArea),
            Some("AXWindow") => Some(Role::Window),
            _ => None,
        };
        nodes.push(Node {
            role,
            names,
            identifier: node.text("AXIdentifier")?,
            parent,
        });
        stack.extend(
            node.children("AXChildren")?
                .into_iter()
                .map(|child| (child, depth + 1, Some(index))),
        );
        elements.push(node);
    }
    let detection = adapter.detect(&nodes).map_err(|error| match error {
        AdapterError::Ambiguous => ProbeError::AmbiguousTarget,
        AdapterError::UnknownState => ProbeError::LoginFormUnavailable,
        _ => ProbeError::UnsupportedAttribute,
    })?;
    if detection.state.kind != StateKind::PasswordLogin {
        return Err(ProbeError::UserActionRequired);
    }
    let selected = |control| {
        detection
            .controls
            .get(&control)
            .and_then(|index| elements.get(*index))
            .cloned()
            .ok_or(ProbeError::LoginFormUnavailable)
    };
    let username = selected(Control::Username)?;
    let password = selected(Control::Password)?;
    let submit = selected(Control::Submit)?;
    let mut geometry = vec![];
    for node in [&window, &username, &password, &submit] {
        geometry.push(node.value("AXPosition")?);
        geometry.push(node.value("AXSize")?);
    }
    Ok(Form {
        window,
        username,
        password,
        submit,
        geometry,
        state_id: detection.state.id.clone(),
        actions: detection.state.actions.clone(),
    })
}

/// Bound to a retained process object and specific window/controls. Not a signature
/// attestation: this fixture-only API must not be reused for real credentials.
pub struct NativeLoginProbe {
    _lease: Lease,
    running: Retained<NSRunningApplication>,
    app: Ax,
    form: Form,
    summary: FormSummary,
    deadline: Instant,
    attestation: Option<autologin_core::VerifiedApplication>,
    adapter: autologin_core::adapter::AppDefinition,
}

/// Reads metadata only. Does not launch an application or ask macOS to grant permission.
pub fn inspect_adapter(
    adapter: &autologin_core::adapter::AppDefinition,
) -> Result<NativeLoginProbe, ProbeError> {
    if !adapter.supports(autologin_core::Platform::Macos, adapter.bundle_id()) {
        return Err(ProbeError::UnsupportedAttribute);
    }
    let lease = Lease::acquire()?;
    if !unsafe { AXIsProcessTrusted() } {
        return Err(ProbeError::PermissionRequired);
    }
    let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(
        adapter.bundle_id(),
    ));
    if apps.is_empty() {
        return Err(ProbeError::AppUnavailable);
    }
    if apps.len() != 1 {
        return Err(ProbeError::AmbiguousTarget);
    }
    let running = apps.objectAtIndex(0);
    let pid = running.processIdentifier();
    let raw = unsafe { AXUIElementCreateApplication(pid) };
    if raw.is_null() {
        return Err(ProbeError::AppUnavailable);
    }
    let app = Ax::from_value(unsafe { CFType::wrap_under_create_rule(raw) })?;
    check(unsafe { AXUIElementSetMessagingTimeout(app.raw(), 0.2) })?;
    let form = recognize(&app, adapter)?;
    let summary = FormSummary {
        process_id: pid,
        username_writable: form.username.writable()?,
        password_writable: form.password.writable()?,
        submit_enabled: form.submit.flag("AXEnabled")?,
        frontmost: is_frontmost(pid),
    };
    Ok(NativeLoginProbe {
        _lease: lease,
        running,
        app,
        form,
        summary,
        deadline: Instant::now() + Duration::from_secs(30),
        attestation: None,
        adapter: adapter.clone(),
    })
}
#[repr(C)]
struct ProcessSerialNumber {
    high: u32,
    low: u32,
}
fn is_frontmost(pid: i32) -> bool {
    // Query WindowServer's live foreground process. Cocoa workspace notifications
    // may not have been delivered on this worker during an application restart.
    let mut process = ProcessSerialNumber { high: 0, low: 0 };
    let mut current = 0;
    unsafe {
        GetFrontProcess(&mut process) == 0
            && GetProcessPID(&process, &mut current) == 0
            && current == pid
    }
}

impl NativeLoginProbe {
    pub fn summary(&self) -> FormSummary {
        self.summary
    }
    /// Consumes the binding; failed/interrupted runs cannot be blindly replayed.
    pub fn exercise_fixture(self, cancel: &Cancellation) -> ProbeReport {
        exercise(&self, cancel)
    }
    fn field(&self, field: Field) -> &Ax {
        match field {
            Field::Username => &self.form.username,
            Field::Password => &self.form.password,
        }
    }
    fn quick_guard(&self) -> Result<(), ProbeError> {
        if Instant::now() > self.deadline {
            return Err(ProbeError::Timeout);
        }
        if self.running.isTerminated() {
            return Err(ProbeError::TargetChanged);
        }
        if let Some(verified) = &self.attestation {
            crate::app_catalog::revalidate_fill_process(verified)
                .map_err(|_| ProbeError::IdentityChanged)?;
        }
        if !is_frontmost(self.summary.process_id) {
            return Err(ProbeError::NotFrontmost);
        }
        if Ax::from_value(self.app.value("AXFocusedWindow")?)? != self.form.window {
            return Err(ProbeError::TargetChanged);
        }
        Ok(())
    }
}
impl FixtureTarget for NativeLoginProbe {
    fn guard(&self) -> Result<(), ProbeError> {
        self.quick_guard()?;
        let current = recognize(&self.app, &self.adapter)?;
        if current.window != self.form.window
            || current.username != self.form.username
            || current.password != self.form.password
            || current.submit != self.form.submit
        {
            return Err(ProbeError::TargetChanged);
        }
        if current.geometry != self.form.geometry {
            return Err(ProbeError::LayoutChanged);
        }
        self.quick_guard()
    }
    fn ensure_empty(&self) -> Result<(), ProbeError> {
        if !self.form.username.empty()? || !self.form.password.empty()? {
            return Err(ProbeError::NotEmpty);
        }
        Ok(())
    }
    fn write(&self, field: Field, text: &str, cancel: &Cancellation) -> Result<(), ProbeError> {
        let node = self.field(field);
        cancel.check()?;
        self.guard()?;
        if !node.writable()? || !node.flag("AXEnabled")? {
            return Err(ProbeError::UnsupportedAttribute);
        }
        let previous_focus = Ax::from_value(self.app.value("AXFocusedUIElement")?)?;
        node.set("AXFocused", &CFBoolean::true_value())?;
        let focus_deadline = Instant::now() + Duration::from_secs(1);
        loop {
            cancel.check()?;
            self.guard()?;
            let current_focus = Ax::from_value(self.app.value("AXFocusedUIElement")?)?;
            if focus_transition(&current_focus, &previous_focus, node)? {
                break;
            }
            if Instant::now() > focus_deadline {
                return Err(ProbeError::FocusNotConfirmed);
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        cancel.check()?;
        self.quick_guard()?;
        if self.attestation.is_some() && !node.empty()? {
            return Err(ProbeError::NotEmpty);
        }
        cancel.check()?;
        node.set("AXValue", &CFString::new(text))
    }
    fn verify(&self, field: Field, empty: bool, cancel: &Cancellation) -> Result<(), ProbeError> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            cancel.check()?;
            self.guard()?;
            let node = self.field(field);
            let matches = if empty {
                node.empty()?
            } else {
                match field {
                    Field::Username => node.text("AXValue")?.as_deref() == Some("0000000000"),
                    // A nonempty secure AX value is only an AX acknowledgement, not
                    // proof of exact password contents or application submission state.
                    Field::Password => !node.empty()?,
                }
            };
            if matches {
                return Ok(());
            }
            if Instant::now() > deadline {
                return Err(ProbeError::Timeout);
            }
            std::thread::sleep(Duration::from_millis(40));
        }
    }
}

/// Production binding. Constructed only from the backend's verified launch result.
/// Owns native references on the worker thread; never serializes them to JavaScript.
pub struct NativeFill(NativeLoginProbe);
impl NativeFill {
    pub fn bind(
        verified: &autologin_core::VerifiedApplication,
        adapter: &autologin_core::adapter::AppDefinition,
    ) -> Result<Self, ProbeError> {
        if !adapter.supports(
            verified.application.platform,
            &verified.application.platform_application_id,
        ) {
            return Err(ProbeError::IdentityChanged);
        }
        crate::app_catalog::revalidate_fill_process(verified)
            .map_err(|_| ProbeError::IdentityChanged)?;
        let mut probe = inspect_adapter(adapter)?;
        if probe.summary.process_id as u32 != verified.launched_process_id {
            return Err(ProbeError::IdentityChanged);
        }
        probe.attestation = Some(verified.clone());
        probe.deadline = Instant::now() + Duration::from_secs(180);
        let fill = Self(probe);
        fill.validate_preparation()?;
        Ok(fill)
    }
    /// Window-relative rectangles for a local schematic, not an uploaded screenshot.
    pub fn preview(&self) -> Result<[[f64; 4]; 3], ProbeError> {
        let window = rectangle(&self.0.form.window)?;
        let mut result = [[0.; 4]; 3];
        for (i, node) in [
            &self.0.form.username,
            &self.0.form.password,
            &self.0.form.submit,
        ]
        .into_iter()
        .enumerate()
        {
            let r = rectangle(node)?;
            result[i] = [
                (r[0] - window[0]) / window[2],
                (r[1] - window[1]) / window[3],
                r[2] / window[2],
                r[3] / window[3],
            ];
            if result[i]
                .iter()
                .any(|v| !v.is_finite() || *v < 0. || *v > 1.)
            {
                return Err(ProbeError::LayoutChanged);
            }
        }
        Ok(result)
    }
    pub fn validate_preparation(&self) -> Result<(), ProbeError> {
        if Instant::now() > self.0.deadline {
            return Err(ProbeError::Timeout);
        }
        if self.0.running.isTerminated() {
            return Err(ProbeError::TargetChanged);
        }
        crate::app_catalog::revalidate_fill_process(
            self.0
                .attestation
                .as_ref()
                .ok_or(ProbeError::IdentityChanged)?,
        )
        .map_err(|_| ProbeError::IdentityChanged)?;
        if recognize(&self.0.app, &self.0.adapter)? != self.0.form {
            return Err(ProbeError::TargetChanged);
        }
        self.0.ensure_empty()
    }
    /// Called after the user explicitly continues. No keys or AX text are sent here.
    #[allow(deprecated)]
    pub fn activate(&self, cancel: &Cancellation) -> Result<(), ProbeError> {
        cancel.check()?;
        self.validate_preparation()?;
        use objc2_app_kit::NSApplicationActivationOptions;
        if !self
            .0
            .running
            .activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps)
        {
            return Err(ProbeError::NotFrontmost);
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while !is_frontmost(self.0.summary.process_id) {
            cancel.check()?;
            if Instant::now() > deadline {
                return Err(ProbeError::NotFrontmost);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.0.guard()
    }
    /// Never presses submit or changes options. Progress flags mean attempted input,
    /// including calls whose result is uncertain. They never contain credential values.
    pub fn fill(
        &self,
        username: &str,
        password: &secrecy::SecretString,
        cancel: &Cancellation,
        mut progress: impl FnMut(Field),
    ) -> Result<(), ProbeError> {
        use secrecy::ExposeSecret;
        cancel.check()?;
        self.0.guard()?;
        self.0.ensure_empty()?;
        let monitor = FocusMonitor::start(self.0.summary.process_id, cancel.clone())?;
        for action in &self.0.form.actions {
            use autologin_core::adapter::{Action, CredentialField};
            let (field, text) = match action {
                Action::Fill {
                    field: CredentialField::Username,
                } => (Field::Username, username),
                Action::Fill {
                    field: CredentialField::Password,
                } => (Field::Password, password.expose_secret()),
                Action::RequestUser => return Err(ProbeError::UserActionRequired),
            };
            cancel.check()?;
            monitor.check()?;
            self.0.guard()?;
            // Do not deliver a password if the user changed the account after our first write.
            if field == Field::Password
                && self.0.field(Field::Username).text("AXValue")?.as_deref() != Some(username)
            {
                return Err(ProbeError::TargetChanged);
            }
            // Never overwrite input the user supplied while preparing the other field.
            if !self.0.field(field).empty()? {
                return Err(ProbeError::NotEmpty);
            }
            progress(field);
            self.0.write(field, text, cancel)?;
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                cancel.check()?;
                monitor.check()?;
                self.0.guard()?;
                let confirmed = match field {
                    Field::Username => {
                        self.0.field(field).text("AXValue")?.as_deref() == Some(username)
                    }
                    Field::Password => !self.0.field(field).empty()?,
                };
                if confirmed {
                    break;
                }
                if Instant::now() > deadline {
                    return Err(ProbeError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        monitor.check()?;
        cancel.check()
    }
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXValueGetValue(value: CFTypeRef, value_type: i32, destination: *mut c_void) -> bool;
}
fn rectangle(node: &Ax) -> Result<[f64; 4], ProbeError> {
    let mut point = [0f64; 2];
    let mut size = [0f64; 2];
    let position = node.value("AXPosition")?;
    let dimensions = node.value("AXSize")?;
    if !unsafe { AXValueGetValue(position.as_CFTypeRef(), 1, point.as_mut_ptr().cast()) }
        || !unsafe { AXValueGetValue(dimensions.as_CFTypeRef(), 2, size.as_mut_ptr().cast()) }
        || size.iter().any(|v| !v.is_finite() || *v <= 0.)
        || point.iter().any(|v| !v.is_finite())
    {
        return Err(ProbeError::UnsupportedAttribute);
    }
    Ok([point[0], point[1], size[0], size[1]])
}

// Sticky monitoring during input: a detected departure cannot be erased by switching
// back. This supplements per-operation AX checks; OS calls already in flight cannot
// be recalled. Independent AX references stay on the monitoring thread.
struct FocusMonitor {
    stop: std::sync::Arc<AtomicBool>,
    changed: std::sync::Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl FocusMonitor {
    fn start(pid: i32, cancel: Cancellation) -> Result<Self, ProbeError> {
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let changed = std::sync::Arc::new(AtomicBool::new(false));
        let (send, recv) = std::sync::mpsc::sync_channel(1);
        let (thread_stop, thread_changed) = (stop.clone(), changed.clone());
        let thread = std::thread::spawn(move || {
            objc2::rc::autoreleasepool(|_| {
                let setup = (|| {
                    let raw = unsafe { AXUIElementCreateApplication(pid) };
                    if raw.is_null() {
                        return Err(ProbeError::AppUnavailable);
                    }
                    let app = Ax::from_value(unsafe { CFType::wrap_under_create_rule(raw) })?;
                    check(unsafe { AXUIElementSetMessagingTimeout(app.raw(), 0.1) })?;
                    let window = Ax::from_value(app.value("AXFocusedWindow")?)?;
                    if !is_frontmost(pid) {
                        return Err(ProbeError::NotFrontmost);
                    }
                    Ok((app, window))
                })();
                let Ok((app, window)) = setup else {
                    let _ = send.send(false);
                    return;
                };
                let _ = send.send(true);
                while !thread_stop.load(Ordering::SeqCst) {
                    if !is_frontmost(pid)
                        || app
                            .value("AXFocusedWindow")
                            .and_then(Ax::from_value)
                            .as_ref()
                            != Ok(&window)
                    {
                        thread_changed.store(true, Ordering::SeqCst);
                        cancel.cancel();
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            });
        });
        let result = Self {
            stop,
            changed,
            thread: Some(thread),
        };
        if recv.recv_timeout(Duration::from_secs(2)) != Ok(true) {
            return Err(ProbeError::NotFrontmost);
        }
        Ok(result)
    }
    fn check(&self) -> Result<(), ProbeError> {
        if self.changed.load(Ordering::SeqCst) {
            Err(ProbeError::NotFrontmost)
        } else {
            Ok(())
        }
    }
}
impl Drop for FocusMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

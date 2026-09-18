//! Native implementation of the v2 driver. No application-specific menu paths.
mod pointer;
use super::*;
use autologin_core::{
    adapter::{CredentialField, Node, Role},
    switcher::{ClickMethod, Definition, Driver, Error, Snapshot},
    VerifiedApplication,
};
use secrecy::{ExposeSecret, SecretString};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementPerformAction(element: AxRef, action: CFStringRef) -> i32;
    fn kill(pid: i32, signal: i32) -> i32;
}
fn process_exited(running: &NSRunningApplication) -> bool {
    // A worker may not have delivered Cocoa lifecycle notifications yet.
    // Signal 0 only probes existence; ESRCH is distinct from lack of permission.
    running.isTerminated()
        || (unsafe { kill(running.processIdentifier(), 0) } == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(3))
}
struct Observation {
    window: Ax,
    elements: Vec<Ax>,
    nodes: Vec<Node>,
    geometry: Vec<Option<[f64; 4]>>,
}
/// Construct on the execution worker after consent and foreground activation.
/// `account_guard` checks that the selected account is unchanged. `credential`
/// retrieves only that account's field, after the task's credential consent.
/// Neither callback nor any credential is stored in the adapter definition.
pub struct NativeWorkflow<C, G> {
    _lease: Lease,
    running: Retained<NSRunningApplication>,
    app: Ax,
    verified: VerifiedApplication,
    definition: Definition,
    cancel: Cancellation,
    credential: C,
    account_guard: G,
    generation: u64,
    observation: Option<Observation>,
    used: bool,
    username: Option<(Ax, SecretString)>,
}
impl<C, G> NativeWorkflow<C, G>
where
    C: FnMut(CredentialField) -> Result<SecretString, Error>,
    G: FnMut() -> Result<(), Error>,
{
    pub fn bind(
        verified: &VerifiedApplication,
        definition: &Definition,
        cancel: Cancellation,
        credential: C,
        account_guard: G,
    ) -> Result<Self, Error> {
        if definition.platform != verified.application.platform
            || definition.application_id != verified.application.platform_application_id
        {
            return Err(Error::TargetChanged);
        }
        definition.to_json()?;
        let lease = Lease::acquire().map_err(map)?;
        if !unsafe { AXIsProcessTrusted() } {
            return Err(Error::PermissionRequired);
        }
        crate::app_catalog::revalidate_fill_process(verified).map_err(|_| Error::TargetChanged)?;
        let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(
            &NSString::from_str(&definition.application_id),
        );
        if apps.len() != 1 {
            return Err(Error::Ambiguous);
        }
        let running = apps.objectAtIndex(0);
        if running.processIdentifier() as u32 != verified.launched_process_id {
            return Err(Error::TargetChanged);
        }
        let raw = unsafe { AXUIElementCreateApplication(running.processIdentifier()) };
        if raw.is_null() {
            return Err(Error::Driver);
        }
        let app = Ax::from_value(unsafe { CFType::wrap_under_create_rule(raw) }).map_err(map)?;
        check(unsafe { AXUIElementSetMessagingTimeout(app.raw(), 0.2) }).map_err(map)?;
        request_accessibility(&app);
        let mut driver = Self {
            _lease: lease,
            running,
            app,
            verified: verified.clone(),
            definition: definition.clone(),
            cancel,
            credential,
            account_guard,
            generation: 0,
            observation: None,
            used: true,
            username: None,
        };
        driver.guard()?;
        Ok(driver)
    }
    fn inspect(&self) -> Result<Observation, Error> {
        inspect_tree(&self.app, &self.cancel)
    }
    pub fn verified_application(&self) -> &VerifiedApplication {
        &self.verified
    }
    pub fn observed_state(&self) -> Result<String, Error> {
        let observation = self.observation.as_ref().ok_or(Error::WindowUnavailable)?;
        Ok(self.definition.detect(&observation.nodes)?.state)
    }

    /// Initial renderer accessibility can become available after activation.
    /// Wait for a known state without sending input or changing the bound process.
    pub fn observe_ready(&mut self, timeout: Duration) -> Result<Snapshot, Error> {
        let deadline = Instant::now() + timeout;
        loop {
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            match self.observe() {
                Ok(snapshot) => match self.definition.detect(&snapshot.nodes) {
                    Ok(_) => return Ok(snapshot),
                    Err(Error::UnknownState) => {}
                    Err(error) => return Err(error),
                },
                Err(Error::WindowUnavailable) => {}
                Err(error) => return Err(error),
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    fn same_controls(&self, old: &Observation, current: &Observation) -> Result<(), Error> {
        if current.window != old.window {
            return Err(Error::TargetChanged);
        }
        let before = self.definition.detect(&old.nodes)?;
        let after = self.definition.detect(&current.nodes)?;
        if before.state != after.state {
            return Err(Error::TargetChanged);
        }
        // Unrelated chat content can change during a menu click. Revalidate the
        // entire selected state and its native controls, not every business label.
        for (name, previous) in before.controls {
            let next = *after.controls.get(&name).ok_or(Error::TargetChanged)?;
            if old.elements[previous] != current.elements[next]
                || old.geometry[previous] != current.geometry[next]
            {
                return Err(Error::TargetChanged);
            }
        }
        Ok(())
    }
    fn transition_click(
        &mut self,
        snapshot: &Snapshot,
        index: usize,
        timeout: Duration,
        method: ClickMethod,
    ) -> Result<(), Error> {
        let deadline = Instant::now() + timeout;
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        let previous_state = self.definition.detect(&snapshot.nodes)?.state;
        if method == ClickMethod::Pointer {
            self.pointer_press(snapshot, index, Some(deadline))?;
        } else {
            let target = self.target(snapshot, index)?;
            if !matches!(
                snapshot.nodes[index].role,
                Some(Role::Button | Role::MenuItem)
            ) {
                return Err(Error::Driver);
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            self.used = true;
            let status = unsafe {
                AXUIElementPerformAction(
                    target.raw(),
                    CFString::new("AXPress").as_concrete_TypeRef(),
                )
            };
            // A dying process may invalidate the AX reply. Never resend input.
            if !matches!(status, 0 | -25202 | -25204) {
                return Err(Error::Driver);
            }
        }
        let previous_pid = self.running.processIdentifier();
        let previous_url = self.running.bundleURL().ok_or(Error::TargetChanged)?;
        loop {
            self.cancel.check().map_err(map)?;
            (self.account_guard)()?;
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            if !process_exited(&self.running) {
                // Some applications replace only their login renderer/window while
                // keeping the same process. Accept a newly recognized state here;
                // Runner still verifies that it is the recorded next state.
                if is_frontmost(self.running.processIdentifier()) {
                    match self.inspect() {
                        Ok(current) => match self.definition.detect(&current.nodes) {
                            Ok(detected) if detected.state != previous_state => {
                                request_accessibility(&self.app);
                                self.observation = None;
                                self.username = None;
                                self.generation =
                                    self.generation.checked_add(1).ok_or(Error::Driver)?;
                                return self.guard();
                            }
                            Ok(_) | Err(Error::UnknownState) => {}
                            Err(error) => return Err(error),
                        },
                        Err(Error::WindowUnavailable) => {}
                        Err(error) => return Err(error),
                    }
                }
                // During the explicitly expected restart the OS can temporarily
                // focus another app. Send no input until the new verified app
                // has naturally returned to the foreground; never activate it here.
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(
                &NSString::from_str(&self.definition.application_id),
            );
            // Cocoa may retain the just-terminated application in this list.
            // The previous PID was proven exited above; never count it as a
            // second live candidate, and never reuse its AX handles.
            let candidates: Vec<_> = (0..apps.len())
                .map(|i| apps.objectAtIndex(i))
                .filter(|app| app.processIdentifier() != previous_pid && !process_exited(app))
                .collect();
            if candidates.is_empty() {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            if candidates.len() != 1 {
                return Err(Error::Ambiguous);
            }
            let running = candidates.into_iter().next().ok_or(Error::ProcessExited)?;
            if running.processIdentifier() == previous_pid
                || running.bundleURL().as_ref() != Some(&previous_url)
            {
                return Err(Error::TargetChanged);
            }
            let mut verified = self.verified.clone();
            verified.launched_process_id = running.processIdentifier() as u32;
            crate::app_catalog::revalidate_fill_process(&verified)
                .map_err(|_| Error::TargetChanged)?;
            if !is_frontmost(running.processIdentifier()) {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            let raw = unsafe { AXUIElementCreateApplication(running.processIdentifier()) };
            if raw.is_null() {
                return Err(Error::Driver);
            }
            let app =
                Ax::from_value(unsafe { CFType::wrap_under_create_rule(raw) }).map_err(map)?;
            check(unsafe { AXUIElementSetMessagingTimeout(app.raw(), 0.2) }).map_err(map)?;
            request_accessibility(&app);
            self.observation = None;
            self.username = None;
            self.generation = self.generation.checked_add(1).ok_or(Error::Driver)?;
            self.app = app;
            self.running = running;
            self.verified = verified;
            return self.guard();
        }
    }
    fn target(&mut self, snapshot: &Snapshot, index: usize) -> Result<Ax, Error> {
        self.guard()?;
        if self.used || snapshot.token != self.generation {
            return Err(Error::TargetChanged);
        }
        let current = self.inspect()?;
        let old = self.observation.as_ref().ok_or(Error::TargetChanged)?;
        if snapshot.nodes != old.nodes {
            return Err(Error::TargetChanged);
        }
        self.same_controls(old, &current)?;
        if old.geometry.get(index).is_none_or(Option::is_none) {
            return Err(Error::TargetChanged);
        }
        let target = old
            .elements
            .get(index)
            .cloned()
            .ok_or(Error::TargetChanged)?;
        if !target.flag("AXEnabled").map_err(map)? {
            return Err(Error::Driver);
        }
        self.guard()?;
        Ok(target)
    }
}
impl<C, G> Driver for NativeWorkflow<C, G>
where
    C: FnMut(CredentialField) -> Result<SecretString, Error>,
    G: FnMut() -> Result<(), Error>,
{
    fn guard(&mut self) -> Result<(), Error> {
        self.cancel.check().map_err(map)?;
        if process_exited(&self.running) {
            return Err(Error::ProcessExited);
        }
        if !is_frontmost(self.running.processIdentifier()) {
            return Err(Error::FocusChanged);
        }
        crate::app_catalog::revalidate_fill_process(&self.verified).map_err(|_| {
            if process_exited(&self.running) {
                Error::ProcessExited
            } else {
                Error::TargetChanged
            }
        })?;
        (self.account_guard)()
    }
    fn observe(&mut self) -> Result<Snapshot, Error> {
        self.guard()?;
        // Invalidate old handles even when a subsequent observation fails.
        self.used = true;
        let observation = self.inspect()?;
        if self
            .observation
            .as_ref()
            .is_none_or(|old| old.window != observation.window)
        {
            // The renderer may not exist at process creation. Request once
            // again when its actual window first becomes available.
            request_accessibility(&self.app);
        }
        self.guard()?;
        self.generation = self.generation.checked_add(1).ok_or(Error::Driver)?;
        let snapshot = Snapshot {
            token: self.generation,
            nodes: observation.nodes.clone(),
        };
        self.observation = Some(observation);
        self.used = false;
        Ok(snapshot)
    }
    fn click(&mut self, snapshot: &Snapshot, index: usize) -> Result<(), Error> {
        let target = self.target(snapshot, index)?;
        if !matches!(
            snapshot.nodes[index].role,
            Some(Role::Button | Role::MenuItem)
        ) {
            return Err(Error::Driver);
        }
        // Consume before dispatch. A native timeout is not proof of no effect.
        self.used = true;
        check(unsafe {
            AXUIElementPerformAction(target.raw(), CFString::new("AXPress").as_concrete_TypeRef())
        })
        .map_err(map)?;
        self.guard()
    }
    fn click_transitioning(
        &mut self,
        snapshot: &Snapshot,
        index: usize,
        timeout: Duration,
    ) -> Result<(), Error> {
        self.transition_click(snapshot, index, timeout, ClickMethod::Accessibility)
    }
    fn pointer_click(&mut self, snapshot: &Snapshot, index: usize) -> Result<(), Error> {
        self.pointer_press(snapshot, index, None)?;
        self.guard()
    }
    fn pointer_click_transitioning(
        &mut self,
        snapshot: &Snapshot,
        index: usize,
        timeout: Duration,
    ) -> Result<(), Error> {
        self.transition_click(snapshot, index, timeout, ClickMethod::Pointer)
    }
    fn clear(
        &mut self,
        snapshot: &Snapshot,
        index: usize,
        field: CredentialField,
    ) -> Result<(), Error> {
        let expected = match field {
            CredentialField::Username => Role::TextField,
            CredentialField::Password => Role::SecureTextField,
        };
        let target = self.target(snapshot, index)?;
        if snapshot.nodes[index].role != Some(expected) || !target.writable().map_err(map)? {
            return Err(Error::Driver);
        }
        self.guard()?;
        // Consume before mutation: a failed AX reply does not prove the write had no effect.
        self.used = true;
        target.set("AXValue", &CFString::new("")).map_err(map)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            self.guard()?;
            if target.empty().map_err(map)? {
                break;
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if field == CredentialField::Username {
            self.username = None;
        }
        Ok(())
    }
    fn check(&mut self, snapshot: &Snapshot, index: usize) -> Result<(), Error> {
        let target = self.target(snapshot, index)?;
        if snapshot.nodes[index].role != Some(Role::Checkbox) {
            return Err(Error::Driver);
        }
        self.guard()?;
        let selected = target.checked().map_err(map)?;
        // Consume the snapshot even when no click is needed.
        self.used = true;
        if selected {
            return Ok(());
        }
        check(unsafe {
            AXUIElementPerformAction(target.raw(), CFString::new("AXPress").as_concrete_TypeRef())
        })
        .map_err(map)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            self.guard()?;
            if target.checked().map_err(map)? {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn fill(
        &mut self,
        snapshot: &Snapshot,
        index: usize,
        field: CredentialField,
    ) -> Result<(), Error> {
        let expected = match field {
            CredentialField::Username => Role::TextField,
            CredentialField::Password => Role::SecureTextField,
        };
        let target = self.target(snapshot, index)?;
        if snapshot.nodes[index].role != Some(expected)
            || !target.writable().map_err(map)?
            || !target.empty().map_err(map)?
        {
            return Err(Error::Driver);
        }
        let secret = (self.credential)(field)?;
        // System credential authorization may have changed focus or layout.
        self.target(snapshot, index)?;
        if secret.expose_secret().is_empty() || secret.expose_secret().len() > 16384 {
            return Err(Error::Driver);
        }
        let monitor = FocusMonitor::start(self.running.processIdentifier(), self.cancel.clone())
            .map_err(map)?;
        let previous =
            Ax::from_value(self.app.value("AXFocusedUIElement").map_err(map)?).map_err(map)?;
        self.used = true;
        target
            .set("AXFocused", &CFBoolean::true_value())
            .map_err(map)?;
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            self.guard()?;
            monitor.check().map_err(map)?;
            let current =
                Ax::from_value(self.app.value("AXFocusedUIElement").map_err(map)?).map_err(map)?;
            if focus_transition(&current, &previous, &target).map_err(map)? {
                break;
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        // Re-observe the same controls before sending text after focus settles.
        let current = self.inspect()?;
        let old = self.observation.as_ref().ok_or(Error::TargetChanged)?;
        self.same_controls(old, &current)?;
        if let Some((username, expected)) = &self.username {
            if username.text("AXValue").map_err(map)?.as_deref() != Some(expected.expose_secret()) {
                return Err(Error::TargetChanged);
            }
        }
        if !target.empty().map_err(map)? {
            return Err(Error::TargetChanged);
        }
        self.guard()?;
        monitor.check().map_err(map)?;
        target
            .set("AXValue", &CFString::new(secret.expose_secret()))
            .map_err(map)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            self.guard()?;
            monitor.check().map_err(map)?;
            let confirmed = match field {
                CredentialField::Username => {
                    target.text("AXValue").map_err(map)?.as_deref() == Some(secret.expose_secret())
                }
                CredentialField::Password => !target.empty().map_err(map)?,
            };
            if confirmed {
                break;
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if field == CredentialField::Username {
            self.username = Some((target, secret));
        }
        Ok(())
    }
}
fn map(error: ProbeError) -> Error {
    match error {
        ProbeError::PermissionRequired => Error::PermissionRequired,
        ProbeError::Cancelled => Error::Cancelled,
        ProbeError::Timeout => Error::Timeout,
        ProbeError::AmbiguousTarget => Error::Ambiguous,
        ProbeError::IdentityChanged
        | ProbeError::TargetChanged
        | ProbeError::NotFrontmost
        | ProbeError::LayoutChanged
        | ProbeError::FocusNotConfirmed => Error::TargetChanged,
        _ => Error::Driver,
    }
}

fn map_observation(error: ProbeError) -> Error {
    match error {
        ProbeError::Accessibility(-25202 | -25204) => Error::WindowUnavailable,
        other => map(other),
    }
}

fn request_accessibility(app: &Ax) {
    // Request metadata once per binding, not on every observation: some apps
    // report false even after handling this request, which can retrigger setup.
    let _ = app.set("AXManualAccessibility", &CFBoolean::true_value());
}

fn inspect_tree(app: &Ax, cancel: &Cancellation) -> Result<Observation, Error> {
    let windows = app.children("AXWindows").map_err(map_observation)?;
    if windows.is_empty() {
        return Err(Error::WindowUnavailable);
    }
    let window = unique(windows).map_err(map_observation)?;
    let Some(focused) = app.optional("AXFocusedWindow").map_err(map_observation)? else {
        return Err(Error::WindowUnavailable);
    };
    if Ax::from_value(focused).map_err(map_observation)? != window {
        return Err(Error::WindowUnavailable);
    }
    let mut stack = vec![(window.clone(), 0, None)];
    if let Some(menu) = app.optional("AXMenuBar").map_err(map_observation)? {
        stack.push((Ax::from_value(menu).map_err(map_observation)?, 0, None));
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut elements = Vec::new();
    let mut nodes = Vec::new();
    let mut geometry = Vec::new();
    while let Some((element, depth, parent)) = stack.pop() {
        cancel.check().map_err(map_observation)?;
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        if elements.len() >= 512 || depth > 24 {
            return Err(Error::Ambiguous);
        }
        let role = match element.text("AXRole").map_err(map_observation)?.as_deref() {
            Some("AXTextField")
                if element
                    .text("AXSubrole")
                    .map_err(map_observation)?
                    .as_deref()
                    == Some("AXSecureTextField") =>
            {
                Some(Role::SecureTextField)
            }
            Some("AXTextField") => Some(Role::TextField),
            Some("AXButton") => Some(Role::Button),
            Some("AXCheckBox") => Some(Role::Checkbox),
            Some("AXMenuItem" | "AXMenuBarItem") => Some(Role::MenuItem),
            Some("AXMenu" | "AXMenuBar") => Some(Role::Menu),
            Some("AXStaticText") => Some(Role::StaticText),
            Some("AXGroup") => Some(Role::Group),
            Some("AXWebArea") => Some(Role::WebArea),
            Some("AXWindow") => Some(Role::Window),
            _ => None,
        };
        let mut names = Vec::new();
        for attribute in ["AXTitle", "AXDescription", "AXPlaceholderValue"] {
            if let Some(value) = element.text(attribute).map_err(map_observation)? {
                if value.len() > 4096 {
                    return Err(Error::InvalidTree);
                }
                names.push(value);
            }
        }
        let identifier = element.text("AXIdentifier").map_err(map_observation)?;
        if excluded_system_menu(role, &names, identifier.as_deref()) {
            // The application AXMenuBar also contains the OS-owned Apple menu.
            // Do not expose or traverse system logout/power/lock actions.
            continue;
        }
        let index = elements.len();
        nodes.push(Node {
            role,
            names,
            identifier,
            parent,
        });
        stack.extend(
            element
                .children("AXChildren")
                .map_err(map_observation)?
                .into_iter()
                .map(|child| (child, depth + 1, Some(index))),
        );
        geometry.push(rectangle(&element).ok());
        elements.push(element);
    }
    Ok(Observation {
        window,
        elements,
        nodes,
        geometry,
    })
}

/// Read-only, local Builder capture. No AXValue reads, credentials or actions.
/// Caller explicitly brings the attested application to the foreground first.
pub fn capture_workflow_nodes(verified: &VerifiedApplication) -> Result<Vec<Node>, Error> {
    Ok(capture_builder(verified, false)?.nodes)
}

/// One local snapshot plus explicit hit-test candidates, nearest element first.
/// Coordinates are transient preview data and must never become selectors.
pub struct PointerCapture {
    pub nodes: Vec<Node>,
    pub candidates: Vec<usize>,
    pub rectangles: Vec<Option<[f64; 4]>>,
}

#[repr(C)]
struct PointerPoint {
    x: f64,
    y: f64,
}
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn CGEventCreate(source: *const c_void) -> CFTypeRef;
    fn CGEventGetLocation(event: CFTypeRef) -> PointerPoint;
    fn AXUIElementCopyElementAtPosition(app: AxRef, x: f32, y: f32, result: *mut AxRef) -> i32;
}

pub fn capture_workflow_pointer(verified: &VerifiedApplication) -> Result<PointerCapture, Error> {
    capture_builder(verified, true)
}

fn capture_builder(verified: &VerifiedApplication, pick: bool) -> Result<PointerCapture, Error> {
    let _lease = Lease::acquire().map_err(map)?;
    if !unsafe { AXIsProcessTrusted() } {
        return Err(Error::PermissionRequired);
    }
    crate::app_catalog::revalidate_fill_process(verified).map_err(|_| Error::TargetChanged)?;
    let pid = verified.launched_process_id as i32;
    let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(
        &verified.application.platform_application_id,
    ));
    if apps.len() != 1 {
        return Err(Error::Ambiguous);
    }
    if apps.objectAtIndex(0).processIdentifier() != pid || !is_frontmost(pid) {
        return Err(Error::TargetChanged);
    }
    let raw = unsafe { AXUIElementCreateApplication(pid) };
    if raw.is_null() {
        return Err(Error::Driver);
    }
    let app = Ax::from_value(unsafe { CFType::wrap_under_create_rule(raw) }).map_err(map)?;
    check(unsafe { AXUIElementSetMessagingTimeout(app.raw(), 0.2) }).map_err(map)?;
    request_accessibility(&app);
    let hit = if pick {
        let raw_event = unsafe { CGEventCreate(ptr::null()) };
        if raw_event.is_null() {
            return Err(Error::Driver);
        }
        let event = unsafe { CFType::wrap_under_create_rule(raw_event) };
        let point = unsafe { CGEventGetLocation(event.as_CFTypeRef()) };
        let mut raw_hit = ptr::null();
        check(unsafe {
            AXUIElementCopyElementAtPosition(
                app.raw(),
                point.x as f32,
                point.y as f32,
                &mut raw_hit,
            )
        })
        .map_err(map)?;
        if raw_hit.is_null() {
            return Err(Error::UnknownState);
        }
        Some(Ax::from_value(unsafe { CFType::wrap_under_create_rule(raw_hit) }).map_err(map)?)
    } else {
        None
    };
    let observation = inspect_tree(&app, &Cancellation::default())?;
    crate::app_catalog::revalidate_fill_process(verified).map_err(|_| Error::TargetChanged)?;
    if !is_frontmost(pid) {
        return Err(Error::TargetChanged);
    }
    let mut candidates = Vec::new();
    if let Some(hit) = hit {
        let mut current = Some(
            observation
                .elements
                .iter()
                .position(|element| *element == hit)
                .ok_or(Error::UnknownState)?,
        );
        while let Some(index) = current {
            if autologin_core::adapter::Selector::from_node(&observation.nodes, index).is_ok() {
                candidates.push(index);
            }
            current = observation.nodes[index].parent;
        }
        if candidates.is_empty() {
            return Err(Error::UnknownState);
        }
    }
    Ok(PointerCapture {
        nodes: observation.nodes,
        candidates,
        rectangles: observation.geometry,
    })
}

/// Called only after a desktop task's explicit continuation. Activation sends no input.
#[allow(deprecated)]
pub fn activate_workflow_target(
    verified: &VerifiedApplication,
    cancel: &Cancellation,
) -> Result<(), Error> {
    cancel.check().map_err(map)?;
    crate::app_catalog::revalidate_fill_process(verified).map_err(|_| Error::TargetChanged)?;
    let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(
        &verified.application.platform_application_id,
    ));
    if apps.len() != 1 {
        return Err(Error::Ambiguous);
    }
    let app = apps.objectAtIndex(0);
    if app.processIdentifier() as u32 != verified.launched_process_id {
        return Err(Error::TargetChanged);
    }
    use objc2_app_kit::NSApplicationActivationOptions;
    if !app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps) {
        return Err(Error::TargetChanged);
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while !is_frontmost(app.processIdentifier()) {
        cancel.check().map_err(map)?;
        if app.isTerminated() || Instant::now() >= deadline {
            return Err(Error::TargetChanged);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    crate::app_catalog::revalidate_fill_process(verified).map_err(|_| Error::TargetChanged)
}

fn excluded_system_menu(role: Option<Role>, names: &[String], identifier: Option<&str>) -> bool {
    (matches!(role, Some(Role::MenuItem | Role::Menu))
        && names
            .iter()
            .any(|name| name == "Apple" || name == "\u{f8ff}"))
        || matches!(
            identifier,
            Some(
                "_logOutNowRequested:"
                    | "_logOutRequested:"
                    | "_lockScreenRequested:"
                    | "_shutDownNowRequested:"
                    | "_shutDownRequested:"
                    | "_restartNowRequested:"
                    | "_restartRequested:"
                    | "_sleepRequested:"
                    | "_forceQuitRequested:"
                    | "_forceQuitPanelRequested:"
                    | "terminate:"
            )
        )
}
#[cfg(test)]
mod builder_scope_tests {
    use super::*;
    #[test]
    fn system_account_logout_is_never_an_application_logout_selector() {
        assert!(excluded_system_menu(
            Some(Role::MenuItem),
            &["Apple".into()],
            None
        ));
        assert!(excluded_system_menu(
            Some(Role::MenuItem),
            &["退出登录".into()],
            Some("_logOutRequested:")
        ));
        assert!(excluded_system_menu(
            Some(Role::MenuItem),
            &["Quit QQ".into()],
            Some("terminate:")
        ));
        assert!(!excluded_system_menu(
            Some(Role::MenuItem),
            &["退出登录".into()],
            Some("account-sign-out")
        ));
    }
}

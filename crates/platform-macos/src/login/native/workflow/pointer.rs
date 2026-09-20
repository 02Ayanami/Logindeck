//! Explicit pointer dispatch against a freshly validated accessibility target.
use super::*;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> AxRef;
    fn AXUIElementGetPid(element: AxRef, pid: *mut i32) -> i32;
    fn CGPreflightPostEventAccess() -> bool;
    fn CGEventCreateMouseEvent(
        source: *const c_void,
        kind: u32,
        point: PointerPoint,
        button: u32,
    ) -> CFTypeRef;
    fn CGEventSetIntegerValueField(event: CFTypeRef, field: u32, value: i64);
    fn CGEventSetFlags(event: CFTypeRef, flags: u64);
    fn CGEventPost(tap: u32, event: CFTypeRef);
    fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
    fn CGEventSourceFlagsState(state: i32) -> u64;
}

fn center(rect: [f64; 4]) -> Result<PointerPoint, Error> {
    if rect.iter().any(|v| !v.is_finite()) || rect[2] <= 0. || rect[3] <= 0. {
        return Err(Error::TargetChanged);
    }
    let point = PointerPoint {
        x: rect[0] + rect[2] / 2.,
        y: rect[1] + rect[3] / 2.,
    };
    if !point.x.is_finite() || !point.y.is_finite() {
        return Err(Error::TargetChanged);
    }
    Ok(point)
}

fn belongs_to_target(nodes: &[Node], hit: usize, target: usize) -> bool {
    let mut current = Some(hit);
    for _ in 0..=24 {
        let Some(index) = current else {
            return false;
        };
        let Some(node) = nodes.get(index) else {
            return false;
        };
        if index == target {
            return true;
        }
        current = node.parent;
    }
    false
}

fn input_idle() -> Result<(), Error> {
    // Do not turn a user's drag or modifier chord into an unrelated operation.
    let modifiers = (1 << 17) | (1 << 18) | (1 << 19) | (1 << 20);
    if (0..3).any(|button| unsafe { CGEventSourceButtonState(0, button) })
        || unsafe { CGEventSourceFlagsState(0) } & modifiers != 0
    {
        return Err(Error::InputBusy);
    }
    Ok(())
}

impl<C, G> NativeWorkflow<C, G>
where
    C: FnMut(CredentialField) -> Result<SecretString, Error>,
    G: FnMut() -> Result<(), Error>,
{
    fn check_pointer_hit(&self, point: &PointerPoint, index: usize) -> Result<(), Error> {
        let raw = unsafe { AXUIElementCreateSystemWide() };
        if raw.is_null() {
            return Err(Error::Driver);
        }
        let system = Ax::from_value(unsafe { CFType::wrap_under_create_rule(raw) }).map_err(map)?;
        check(unsafe { AXUIElementSetMessagingTimeout(system.raw(), 0.2) }).map_err(map)?;
        let mut hit = ptr::null();
        check(unsafe {
            AXUIElementCopyElementAtPosition(system.raw(), point.x as f32, point.y as f32, &mut hit)
        })
        .map_err(map)?;
        if hit.is_null() {
            return Err(Error::TargetObscured);
        }
        let hit = Ax::from_value(unsafe { CFType::wrap_under_create_rule(hit) }).map_err(map)?;
        let mut pid = 0;
        check(unsafe { AXUIElementGetPid(hit.raw(), &mut pid) }).map_err(map)?;
        if pid != self.running.processIdentifier() {
            return Err(Error::TargetObscured);
        }
        let observation = self.observation.as_ref().ok_or(Error::TargetChanged)?;
        let hit_index = observation
            .elements
            .iter()
            .position(|element| *element == hit)
            .ok_or(Error::TargetObscured)?;
        if !belongs_to_target(&observation.nodes, hit_index, index) {
            return Err(Error::TargetObscured);
        }
        Ok(())
    }

    /// Consumes exactly one snapshot and posts one down/up pair. No AXPress fallback.
    pub(super) fn pointer_press(
        &mut self,
        snapshot: &Snapshot,
        index: usize,
        deadline: Option<Instant>,
    ) -> Result<(), Error> {
        if !unsafe { CGPreflightPostEventAccess() } {
            return Err(Error::PermissionRequired);
        }
        let target = self.target(snapshot, index)?;
        if !matches!(
            snapshot.nodes[index].role,
            Some(Role::Button | Role::MenuItem)
        ) {
            return Err(Error::Driver);
        }
        let rect = rectangle(&target).map_err(map)?;
        let point = center(rect)?;
        let event = |kind| -> Result<CFType, Error> {
            let raw = unsafe {
                CGEventCreateMouseEvent(
                    ptr::null(),
                    kind,
                    PointerPoint {
                        x: point.x,
                        y: point.y,
                    },
                    0,
                )
            };
            if raw.is_null() {
                return Err(Error::Driver);
            }
            let event = unsafe { CFType::wrap_under_create_rule(raw) };
            unsafe {
                CGEventSetIntegerValueField(event.as_CFTypeRef(), 1, 1); // kCGMouseEventClickState
                CGEventSetFlags(event.as_CFTypeRef(), 0);
            }
            Ok(event)
        };
        let down = event(1)?; // kCGEventLeftMouseDown
        let up = event(2)?; // kCGEventLeftMouseUp
                            // Revalidate after allocation and immediately before the global input.
        let current = self.target(snapshot, index)?;
        if rectangle(&current).map_err(map)? != rect {
            return Err(Error::TargetChanged);
        }
        self.check_pointer_hit(&point, index)?;
        self.guard()?;
        input_idle()?;
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(Error::Timeout);
        }
        self.used = true;
        unsafe {
            CGEventPost(0, down.as_CFTypeRef()); // kCGHIDEventTap
                                                 // Always release, even if the down event changes focus or exits the app.
            CGEventPost(0, up.as_CFTypeRef());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pointer_geometry_and_descendant_guards_reject_unrelated_targets() {
        assert!(center([0., 0., 0., 10.]).is_err());
        assert!(center([f64::NAN, 0., 10., 10.]).is_err());
        let p = center([-200., 40., 60., 20.]).unwrap();
        assert_eq!((p.x, p.y), (-170., 50.));
        let node = |parent| Node {
            role: None,
            names: vec![],
            identifier: None,
            parent,
        };
        let nodes = vec![node(None), node(Some(0)), node(Some(1)), node(Some(0))];
        assert!(belongs_to_target(&nodes, 2, 1));
        assert!(belongs_to_target(&nodes, 1, 1));
        assert!(!belongs_to_target(&nodes, 3, 1));
        assert!(!belongs_to_target(&nodes, 0, 1));
        assert!(!belongs_to_target(&nodes, 99, 1));
    }
}

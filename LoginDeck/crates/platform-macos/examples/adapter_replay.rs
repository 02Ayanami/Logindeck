//! Local v2 diagnostics and explicit logout-only replay, without credentials.
#[cfg(target_os = "macos")]
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
#[cfg(target_os = "macos")]
fn run() -> Result<(), String> {
    use autologin_core::{
        switcher::{Consent, Definition, Driver, FlowKind, Outcome, Progress, Runner, Step},
        ApplicationCatalog, ApplicationRecord,
    };
    use platform_macos::{
        login::{activate_workflow_target, Cancellation, NativeWorkflow},
        MacApplicationCatalog,
    };
    use std::{
        io::Read,
        time::{Duration, Instant},
    };
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !matches!(args.len(), 2 | 3 | 5) || (args.len() == 5 && args[3] != "--step") {
        return Err("Usage: adapter_replay APP_PATH CONFIG_JSON [LOGOUT_FLOW [--step INDEX]] (omitting flow observes only; step runs one configured click and verifies its next state)".into());
    }
    let mut source = String::new();
    std::fs::File::open(&args[1])
        .map_err(|_| "config unavailable")?
        .take(65537)
        .read_to_string(&mut source)
        .map_err(|_| "invalid config")?;
    let definition = Definition::parse(&source).map_err(|e| format!("{e:?}"))?;
    if let Some(id) = args.get(2) {
        let flow = definition.flows.get(id).ok_or("unknown flow")?;
        if flow.kind != FlowKind::Logout
            || flow.outcome != Outcome::LoggedOut
            || flow.steps.iter().any(|s| {
                matches!(
                    s,
                    Step::Clear { .. }
                        | Step::Fill { .. }
                        | Step::Submit { .. }
                        | Step::Challenge { .. }
                )
            })
        {
            return Err("only logout click/wait replay supported".into());
        }
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "runtime")?;
    let catalog = MacApplicationCatalog::new();
    let mut candidates = rt
        .block_on(catalog.import_bundle(std::path::Path::new(&args[0])))
        .map_err(|e| e.code().to_owned())?;
    if candidates.len() != 1 {
        return Err("ambiguous application".into());
    }
    let item = candidates.remove(0);
    let mut app = ApplicationRecord::new(
        item.platform,
        item.platform_application_id,
        item.display_name,
    )
    .map_err(|e| e.code().to_owned())?;
    app.launch_target = item.launch_target;
    app.signature_identity = item.signature_identity;
    app.path_access_ref = item.path_access_ref;
    if definition.platform != app.platform
        || definition.application_id != app.platform_application_id
    {
        return Err("adapter mismatch".into());
    }
    let verified = rt
        .block_on(catalog.verify_and_launch(&app))
        .map_err(|e| e.code().to_owned())?;
    let cancel = Cancellation::default();
    println!("bound_process_id={}", verified.launched_process_id);
    activate_workflow_target(&verified, &cancel).map_err(|e| format!("{e:?}"))?;
    let mut driver = NativeWorkflow::bind(
        &verified,
        &definition,
        cancel,
        |_| Err(autologin_core::switcher::Error::Driver),
        || Ok(()),
    )
    .map_err(|e| format!("{e:?}"))?;
    let snapshot = driver
        .observe_ready(Duration::from_secs(10))
        .map_err(|e| format!("{e:?}"))?;
    let found = definition
        .detect(&snapshot.nodes)
        .map_err(|e| format!("{e:?}"))?;
    println!("detected_state={}", found.state);
    for (name, index) in &found.controls {
        let mut parent = snapshot.nodes[*index].parent;
        let mut roles = Vec::new();
        while let Some(i) = parent {
            roles.push(snapshot.nodes[i].role);
            parent = snapshot.nodes[i].parent;
        }
        println!("control={name} ancestors={roles:?}");
    }
    let Some(flow) = args.get(2) else {
        return Ok(());
    };
    if args.len() == 5 {
        let index: usize = args[4].parse().map_err(|_| "invalid step index")?;
        let selected = definition.flows[flow]
            .steps
            .get(index)
            .ok_or("unknown step")?;
        let Step::Click {
            state,
            target,
            next_states,
            method,
        } = selected
        else {
            return Err("step replay only supports configured logout clicks".into());
        };
        if &found.state != state {
            return Err("current state does not match selected step".into());
        }
        let deadline = Instant::now() + Duration::from_millis(definition.flows[flow].timeout_ms);
        driver
            .click_configured(
                &snapshot,
                found.controls[target],
                *method,
                deadline.saturating_duration_since(Instant::now()),
            )
            .map_err(|e| format!("{e:?}"))?;
        let mut last_state = Some(found.state.clone());
        loop {
            if Instant::now() >= deadline {
                return Err(format!("step {index} transition timed out: expected={next_states:?}, last_observed={}; click was not repeated. Re-record the target or try an application menu-bar action.", last_state.as_deref().unwrap_or("unknown")));
            }
            match driver.observe() {
                Ok(next) => match definition.detect(&next.nodes) {
                    Ok(detected) if next_states.contains(&detected.state) => {
                        println!(
                            "step_complete={index} detected_state={} attempted_actions=1",
                            detected.state
                        );
                        return Ok(());
                    }
                    Ok(detected) => last_state = Some(detected.state),
                    Err(autologin_core::switcher::Error::UnknownState) => last_state = None,
                    Err(e) => return Err(format!("{e:?}")),
                },
                Err(autologin_core::switcher::Error::WindowUnavailable) => {}
                Err(e) => return Err(format!("{e:?}")),
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let started = Instant::now();
    let mut previous = String::new();
    let mut runner = Runner::new(
        definition,
        flow,
        Consent {
            logout: true,
            submit: false,
        },
        Instant::now(),
    )
    .map_err(|e| format!("{e:?}"))?;
    loop {
        let progress = runner.tick(&mut driver, Instant::now());
        let diagnostic = format!(
            "process={} observed={:?} attempted_actions={}",
            driver.verified_application().launched_process_id,
            driver.observed_state(),
            runner.attempted_actions
        );
        if diagnostic != previous {
            println!("elapsed_ms={} {diagnostic}", started.elapsed().as_millis());
            previous = diagnostic;
        }
        match progress {
            Progress::Running => std::thread::sleep(Duration::from_millis(100)),
            Progress::Complete(outcome) => {
                println!(
                    "result={outcome:?} attempted_actions={}",
                    runner.attempted_actions
                );
                return Ok(());
            }
            other => {
                let apps =
                    objc2_app_kit::NSRunningApplication::runningApplicationsWithBundleIdentifier(
                        &objc2_foundation::NSString::from_str(
                            &verified.application.platform_application_id,
                        ),
                    );
                let current: Vec<_> = (0..apps.len())
                    .map(|i| apps.objectAtIndex(i).processIdentifier())
                    .collect();
                eprintln!("current_process_ids={current:?}");
                return Err(format!(
                    "result={other:?} attempted_actions={}",
                    runner.attempted_actions
                ));
            }
        }
    }
}
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macOS only");
    std::process::exit(1);
}

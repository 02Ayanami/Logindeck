use autologin_core::{
    adapter::{CredentialField, Node, Role},
    switcher::*,
};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
fn source() -> Value {
    serde_json::from_str(include_str!(
        "../../../adapters/examples/switcher-v2.fixture.json"
    ))
    .unwrap()
}
fn definition(value: &Value) -> Definition {
    Definition::parse(&value.to_string()).unwrap()
}
fn node(role: Role, name: &str, id: Option<&str>, parent: Option<usize>) -> Node {
    Node {
        role: Some(role),
        names: vec![name.into()],
        identifier: id.map(str::to_owned),
        parent,
    }
}
struct Fake {
    value: Value,
    page: &'static str,
    calls: Vec<String>,
    fail_click: bool,
    fail_clear: bool,
    invalid_guard: bool,
}
impl Fake {
    fn new(value: Value) -> Self {
        Self {
            value,
            page: "home",
            calls: vec![],
            fail_click: false,
            fail_clear: false,
            invalid_guard: false,
        }
    }
}
impl Driver for Fake {
    fn guard(&mut self) -> Result<(), Error> {
        if self.invalid_guard {
            Err(Error::TargetChanged)
        } else {
            Ok(())
        }
    }
    fn observe(&mut self) -> Result<Snapshot, Error> {
        let controls = self.value["states"][self.page]["controls"]
            .as_object()
            .unwrap();
        let mut nodes = vec![
            node(Role::Window, "Window", None, None),
            node(Role::Menu, "", None, Some(0)),
        ];
        for control in controls.values() {
            let role = serde_json::from_value(control["role"].clone()).unwrap();
            nodes.push(node(
                role,
                control["names"][0].as_str().unwrap_or(""),
                control["identifier"].as_str(),
                Some(1),
            ));
        }
        Ok(Snapshot { token: 1, nodes })
    }
    fn click(&mut self, _: &Snapshot, _: usize) -> Result<(), Error> {
        self.calls.push(format!("click:{}", self.page));
        self.page = match self.page {
            "home" => "menu",
            "menu" => "password",
            "password" => "verification",
            _ => panic!(),
        };
        if self.fail_click {
            Err(Error::Timeout)
        } else {
            Ok(())
        }
    }
    fn fill(&mut self, _: &Snapshot, _: usize, field: CredentialField) -> Result<(), Error> {
        self.calls.push(format!("fill:{field:?}"));
        Ok(())
    }
    fn clear(&mut self, _: &Snapshot, _: usize, field: CredentialField) -> Result<(), Error> {
        self.calls.push(format!("clear:{field:?}"));
        if self.fail_clear {
            Err(Error::Driver)
        } else {
            Ok(())
        }
    }
}
#[test]
fn two_apps_switch_using_one_engine_and_only_different_descriptions() {
    for second in [false, true] {
        let mut value = source();
        if second {
            value["id"] = json!("second-macos");
            value["platform"] = json!("macos");
            value["application_id"] = json!("org.example.second");
            value["states"]["home"]["controls"]["account_menu"]["names"] = json!(["Profile"]);
            value["states"]["menu"]["controls"]["logout"]["names"] = json!(["Log off account"]);
        }
        let now = Instant::now();
        let mut driver = Fake::new(value.clone());
        let mut logout = Runner::new(
            definition(&value),
            "logout",
            Consent {
                logout: true,
                submit: false,
            },
            now,
        )
        .unwrap();
        assert_eq!(logout.tick(&mut driver, now), Progress::Running);
        assert_eq!(logout.tick(&mut driver, now), Progress::Running);
        assert_eq!(
            logout.tick(&mut driver, now),
            Progress::Complete(Outcome::LoggedOut)
        );
        assert_eq!(driver.page, "password");
        let mut login = Runner::new(
            definition(&value),
            "login",
            Consent {
                logout: false,
                submit: true,
            },
            now,
        )
        .unwrap();
        for _ in 0..3 {
            assert_eq!(login.tick(&mut driver, now), Progress::Running);
        }
        assert_eq!(login.tick(&mut driver, now), Progress::WaitingForUser);
        driver.page = "home";
        assert_eq!(login.tick(&mut driver, now), Progress::WaitingForUser);
        login.continue_challenge().unwrap();
        assert_eq!(login.continue_challenge(), Err(Error::NotWaiting));
        assert_eq!(
            login.tick(&mut driver, now),
            Progress::Complete(Outcome::LoggedInObserved)
        );
        assert_eq!(
            driver.calls,
            [
                "click:home",
                "click:menu",
                "fill:Username",
                "fill:Password",
                "click:password"
            ]
        );
    }
}
#[test]
fn click_timeout_is_sticky_even_when_native_action_already_happened() {
    let value = source();
    let now = Instant::now();
    let mut driver = Fake::new(value.clone());
    driver.fail_click = true;
    let mut runner = Runner::new(
        definition(&value),
        "logout",
        Consent {
            logout: true,
            submit: false,
        },
        now,
    )
    .unwrap();
    assert_eq!(
        runner.tick(&mut driver, now),
        Progress::Stopped(Error::Timeout)
    );
    assert_eq!(driver.page, "menu");
    driver.fail_click = false;
    assert_eq!(
        runner.tick(&mut driver, now),
        Progress::Stopped(Error::Timeout)
    );
    assert_eq!(runner.attempted_actions, 1);
    assert_eq!(driver.calls.len(), 1);
}
#[test]
fn cancel_target_change_and_deadline_prevent_more_actions() {
    for error in [Error::Cancelled, Error::TargetChanged, Error::Timeout] {
        let value = source();
        let now = Instant::now();
        let mut driver = Fake::new(value.clone());
        let mut runner = Runner::new(
            definition(&value),
            "logout",
            Consent {
                logout: true,
                submit: false,
            },
            now,
        )
        .unwrap();
        runner.tick(&mut driver, now);
        if error == Error::Cancelled {
            runner.cancel();
        }
        if error == Error::TargetChanged {
            driver.invalid_guard = true;
        }
        let next = if error == Error::Timeout {
            now + Duration::from_secs(30)
        } else {
            now
        };
        assert_eq!(runner.tick(&mut driver, next), Progress::Stopped(error));
        assert_eq!(driver.calls.len(), 1);
    }
}
#[test]
fn logout_and_submit_need_separate_consent_and_fill_does_not_submit() {
    let value = source();
    let now = Instant::now();
    assert!(matches!(
        Runner::new(definition(&value), "logout", Consent::default(), now),
        Err(Error::ConsentRequired)
    ));
    assert!(matches!(
        Runner::new(
            definition(&value),
            "login",
            Consent {
                logout: true,
                submit: false
            },
            now
        ),
        Err(Error::ConsentRequired)
    ));
    let mut driver = Fake::new(value.clone());
    driver.page = "password";
    let mut runner = Runner::new(definition(&value), "fill", Consent::default(), now).unwrap();
    runner.tick(&mut driver, now);
    assert_eq!(
        runner.tick(&mut driver, now),
        Progress::Complete(Outcome::Filled)
    );
    runner.tick(&mut driver, now);
    assert_eq!(driver.calls, ["fill:Username", "fill:Password"]);
}
#[test]
fn explicit_clear_runs_once_before_fill_and_does_not_change_filled_outcome_rules() {
    let mut value = source();
    value["flows"]["fill"]["steps"] = json!([
        {"type":"clear", "state":"password", "target":"username", "field":"username"},
        {"type":"clear", "state":"password", "target":"password", "field":"password"},
        {"type":"fill", "state":"password", "target":"username", "field":"username"},
        {"type":"fill", "state":"password", "target":"password", "field":"password"}
    ]);
    let now = Instant::now();
    let mut driver = Fake::new(value.clone());
    driver.page = "password";
    let mut runner = Runner::new(definition(&value), "fill", Consent::default(), now).unwrap();
    for _ in 0..3 {
        assert_eq!(runner.tick(&mut driver, now), Progress::Running);
    }
    assert_eq!(
        runner.tick(&mut driver, now),
        Progress::Complete(Outcome::Filled)
    );
    assert_eq!(runner.attempted_actions, 4);
    assert_eq!(
        driver.calls,
        [
            "clear:Username",
            "clear:Password",
            "fill:Username",
            "fill:Password"
        ]
    );

    value["flows"]["fill"]["steps"] = json!([
        {"type":"clear", "state":"password", "target":"username", "field":"username"}
    ]);
    assert!(Definition::parse(&value.to_string()).is_err());
}
#[test]
fn clear_failure_is_sticky_and_unsupported_drivers_reject_before_mutation() {
    let mut value = source();
    value["flows"]["fill"]["steps"]
        .as_array_mut()
        .unwrap()
        .insert(
            0,
            json!({"type":"clear", "state":"password", "target":"username", "field":"username"}),
        );
    let now = Instant::now();
    let mut driver = Fake::new(value.clone());
    driver.page = "password";
    driver.fail_clear = true;
    let mut runner = Runner::new(definition(&value), "fill", Consent::default(), now).unwrap();
    assert_eq!(
        runner.tick(&mut driver, now),
        Progress::Stopped(Error::Driver)
    );
    assert_eq!(
        runner.tick(&mut driver, now),
        Progress::Stopped(Error::Driver)
    );
    assert_eq!(runner.attempted_actions, 1);
    assert_eq!(driver.calls, ["clear:Username"]);

    struct Unsupported(Fake);
    impl Driver for Unsupported {
        fn guard(&mut self) -> Result<(), Error> {
            self.0.guard()
        }
        fn observe(&mut self) -> Result<Snapshot, Error> {
            self.0.observe()
        }
        fn click(&mut self, snapshot: &Snapshot, node: usize) -> Result<(), Error> {
            self.0.click(snapshot, node)
        }
        fn fill(
            &mut self,
            snapshot: &Snapshot,
            node: usize,
            field: CredentialField,
        ) -> Result<(), Error> {
            self.0.fill(snapshot, node, field)
        }
    }
    let mut unsupported = Unsupported(Fake::new(value.clone()));
    unsupported.0.page = "password";
    let mut runner = Runner::new(definition(&value), "fill", Consent::default(), now).unwrap();
    assert_eq!(
        runner.tick(&mut unsupported, now),
        Progress::Stopped(Error::Driver)
    );
    assert!(unsupported.0.calls.is_empty());
    assert_eq!(runner.attempted_actions, 1);
}
#[test]
fn clear_is_limited_to_login_fields_with_matching_roles() {
    for (state, target, field) in [
        ("home", "account_menu", "username"),
        ("password", "password", "username"),
        ("password", "username", "password"),
    ] {
        let mut value = source();
        value["flows"]["fill"]["steps"]
            .as_array_mut()
            .unwrap()
            .insert(
                0,
                json!({"type":"clear", "state":state, "target":target, "field":field}),
            );
        assert!(Definition::parse(&value.to_string()).is_err());
    }
}
#[test]
fn invalid_paths_secrets_scripts_and_unsafe_password_fields_are_rejected() {
    for (pointer, replacement) in [
        ("/flows/logout/steps/0/target", json!("missing")),
        ("/flows/logout/steps/0/next_states/0", json!("missing")),
        ("/flows/logout/steps/1/state", json!("password")),
        ("/flows/logout/timeout_ms", json!(0)),
        ("/flows/logout/outcome", json!("logged_in_observed")),
        (
            "/states/password/controls/password/role",
            json!("text_field"),
        ),
        ("/flows/login/steps/0/type", json!("shell")),
        ("/schema_version", json!(4)),
    ] {
        let mut value = source();
        *value.pointer_mut(pointer).unwrap() = replacement;
        assert!(Definition::parse(&value.to_string()).is_err(), "{pointer}");
    }
    let mut value = source();
    value["flows"]["fill"]["steps"][1]["password"] = json!("must-not-be-stored");
    assert!(Definition::parse(&value.to_string()).is_err());
}

#[test]
fn login_flow_accepts_an_explicit_same_page_checkbox_step() {
    let mut value = source();
    value["states"]["password"]["controls"]["agreement"] =
        json!({"role":"checkbox", "names":["Accept terms"]});
    value["flows"]["login"]["steps"]
        .as_array_mut()
        .unwrap()
        .insert(
            2,
            json!({"type":"check", "state":"password", "target":"agreement"}),
        );
    assert!(Definition::parse(&value.to_string()).is_ok());
}
#[test]
fn selector_ambiguity_and_malformed_trees_never_pick_a_target() {
    let value = source();
    let adapter = definition(&value);
    let mut driver = Fake::new(value);
    let mut snapshot = driver.observe().unwrap();
    snapshot
        .nodes
        .push(node(Role::Button, "Account menu", None, Some(0)));
    assert!(matches!(
        adapter.detect(&snapshot.nodes),
        Err(Error::Ambiguous)
    ));
    snapshot.nodes[0].parent = Some(0);
    assert!(matches!(
        adapter.detect(&snapshot.nodes),
        Err(Error::InvalidTree)
    ));
}
#[test]
fn configuration_roundtrip_keeps_workflows_data_only() {
    let adapter = definition(&source());
    let exported = adapter.to_json().unwrap();
    let parsed = Definition::parse(&exported).unwrap();
    assert_eq!(parsed.id, adapter.id);
    assert_eq!(parsed.flows.len(), 3);
}

#[test]
fn local_builder_exports_selected_controls_but_not_the_captured_tree() {
    use autologin_core::switcher::builder::Builder;
    let value = source();
    let definition = definition(&value);
    let mut builder = Builder::new(
        "local-fixture".into(),
        autologin_core::Platform::Macos,
        "org.example.demo".into(),
    )
    .unwrap();
    let mut driver = Fake::new(value);
    for page in ["home", "menu", "password", "verification"] {
        driver.page = page;
        let mut snapshot = driver.observe().unwrap();
        let selected: Vec<_> = definition.states[page]
            .controls
            .keys()
            .enumerate()
            .map(|(i, key)| (key.clone(), i + 2))
            .collect();
        snapshot.nodes.push(node(
            Role::StaticText,
            "UNSELECTED_PRIVATE_CONTENT",
            None,
            Some(0),
        ));
        builder
            .record_state(
                page.into(),
                definition.states[page].kind,
                snapshot.nodes,
                &selected,
            )
            .unwrap();
    }
    for (name, flow) in definition.flows {
        builder.set_flow(name, flow).unwrap();
    }
    let exported = builder.export().unwrap();
    assert!(!exported.contains("UNSELECTED_PRIVATE_CONTENT"));
    assert!(Definition::parse(&exported).is_ok());
    // Removing referenced states cannot produce a broken executable configuration.
    builder.remove_state("password");
    assert_eq!(builder.export(), Err(Error::InvalidDefinition));
}
#[test]
fn local_builder_imports_valid_definitions_and_restores_incomplete_bounded_drafts() {
    use autologin_core::switcher::builder::Builder;

    let imported = Builder::from_definition(definition(&source())).unwrap();
    assert_eq!(
        definition(&serde_json::from_str(&imported.export().unwrap()).unwrap()).id,
        "demo-macos"
    );

    let mut draft = imported;
    draft.remove_state("password");
    assert_eq!(draft.export(), Err(Error::InvalidDefinition));
    let source = draft.draft_json().unwrap();
    let restored = Builder::from_draft(&source).unwrap();
    assert!(!restored.states().contains_key("password"));
    assert_eq!(restored.export(), Err(Error::InvalidDefinition));

    let mut unknown: Value = serde_json::from_str(&source).unwrap();
    unknown["unexpected"] = json!(true);
    assert!(Builder::from_draft(&unknown.to_string()).is_err());
    assert!(Builder::from_draft(&"x".repeat(65_537)).is_err());
}
#[test]
fn local_builder_rejects_ambiguous_and_overlapping_recordings() {
    use autologin_core::switcher::builder::Builder;
    let value = source();
    let definition = definition(&value);
    let mut builder = Builder::new(
        "local-fixture".into(),
        autologin_core::Platform::Macos,
        "org.example.demo".into(),
    )
    .unwrap();
    let mut driver = Fake::new(value);
    driver.page = "password";
    let snapshot = driver.observe().unwrap();
    let selected: Vec<_> = definition.states["password"]
        .controls
        .keys()
        .enumerate()
        .map(|(i, key)| (key.clone(), i + 2))
        .collect();
    builder
        .record_state(
            "password".into(),
            StateKind::LoginPage,
            snapshot.nodes.clone(),
            &selected,
        )
        .unwrap();
    builder
        .record_state(
            "same_page".into(),
            StateKind::LoginPage,
            snapshot.nodes,
            &selected,
        )
        .unwrap();
    builder
        .set_flow("fill".into(), definition.flows["fill"].clone())
        .unwrap();
    assert_eq!(builder.export(), Err(Error::Ambiguous));
    let nodes = vec![
        node(Role::Button, "Duplicate", None, None),
        node(Role::Button, "Duplicate", None, None),
    ];
    assert_eq!(
        builder.record_state(
            "duplicate".into(),
            StateKind::Intermediate,
            nodes,
            &[("button".into(), 0)]
        ),
        Err(Error::Ambiguous)
    );
}
#[test]
fn recorded_selector_uses_identifier_or_ancestor_without_coordinates() {
    use autologin_core::adapter::Selector;
    let nodes = vec![
        node(Role::Group, "Account", None, None),
        node(Role::Button, "Continue", None, Some(0)),
        node(Role::Group, "Other", None, None),
        node(Role::Button, "Continue", None, Some(2)),
    ];
    let selector = serde_json::to_value(Selector::from_node(&nodes, 1).unwrap()).unwrap();
    assert_eq!(selector["ancestor"]["names"], json!(["Account"]));
    let nodes = vec![node(
        Role::TextField,
        "Dynamic label",
        Some("stable-user"),
        None,
    )];
    let selector = serde_json::to_value(Selector::from_node(&nodes, 0).unwrap()).unwrap();
    assert_eq!(selector["identifier"], "stable-user");
    assert_eq!(selector["names"], json!([]));
}

#[test]
fn recorded_descendant_selects_outer_button_and_rejects_missing_or_ambiguous_structure() {
    use autologin_core::adapter::Selector;
    let mut nodes = vec![
        node(Role::Group, "", None, None),
        node(Role::Button, "More", None, Some(0)),
        node(Role::Button, "More", None, Some(0)),
        node(Role::Button, "More", None, Some(2)),
    ];
    let selector = serde_json::to_value(Selector::from_node(&nodes, 2).unwrap()).unwrap();
    assert_eq!(selector["descendant"]["role"], "button");
    let mut value = source();
    value["states"]["home"]["controls"] = json!({"account_menu": selector});
    let adapter = definition(&value);
    assert_eq!(adapter.detect(&nodes).unwrap().controls["account_menu"], 2);
    let nested = vec![
        node(Role::Group, "", None, None),
        node(Role::Button, "More", None, Some(0)),
        node(Role::Button, "More", None, Some(0)),
        node(Role::Group, "", None, Some(2)),
        node(Role::Group, "", None, Some(3)),
        node(Role::Button, "More", None, Some(4)),
        node(Role::Group, "", None, Some(1)),
    ];
    assert_eq!(adapter.detect(&nested).unwrap().controls["account_menu"], 2);
    let recorded = serde_json::to_value(Selector::from_node(&nested, 2).unwrap()).unwrap();
    assert_eq!(recorded["descendant"]["role"], "button");
    nodes[3].parent = Some(0);
    assert_eq!(adapter.detect(&nodes).err(), Some(Error::UnknownState));
    nodes[3].parent = Some(2);
    nodes.push(node(Role::Button, "More", None, Some(1)));
    assert_eq!(adapter.detect(&nodes).err(), Some(Error::Ambiguous));
    value["states"]["home"]["controls"]["account_menu"]["descendant"]["names"] =
        json!(["x".repeat(257)]);
    assert!(Definition::parse(&value.to_string()).is_err());
}

#[test]
fn overlay_exclusion_distinguishes_menu_from_background_without_priority_guessing() {
    let mut value = source();
    value["states"]["home"]["absent"] = json!([{"role":"menu_item","names":["Sign out"]}]);
    let adapter = definition(&value);
    let mut driver = Fake::new(value.clone());
    let mut snapshot = driver.observe().unwrap();
    assert_eq!(adapter.detect(&snapshot.nodes).unwrap().state, "home");
    snapshot
        .nodes
        .push(node(Role::MenuItem, "Sign out", None, Some(1)));
    assert_eq!(adapter.detect(&snapshot.nodes).unwrap().state, "menu");
    value["states"]["home"]["absent"] = json!([{"role":"menu_item","names":[]}]);
    assert!(Definition::parse(&value.to_string()).is_err());
}
#[test]
fn expected_window_transition_waits_without_replaying_but_focus_loss_stops() {
    struct Transition {
        driver: Fake,
        unavailable: bool,
        focus_lost: bool,
    }
    impl Driver for Transition {
        fn guard(&mut self) -> Result<(), Error> {
            if self.focus_lost {
                Err(Error::FocusChanged)
            } else {
                self.driver.guard()
            }
        }
        fn observe(&mut self) -> Result<Snapshot, Error> {
            if self.unavailable {
                Err(Error::WindowUnavailable)
            } else {
                self.driver.observe()
            }
        }
        fn click(&mut self, s: &Snapshot, i: usize) -> Result<(), Error> {
            self.driver.click(s, i)
        }
        fn fill(&mut self, s: &Snapshot, i: usize, f: CredentialField) -> Result<(), Error> {
            self.driver.fill(s, i, f)
        }
    }
    let value = source();
    let now = Instant::now();
    let mut driver = Transition {
        driver: Fake::new(value.clone()),
        unavailable: false,
        focus_lost: false,
    };
    let mut runner = Runner::new(
        definition(&value),
        "logout",
        Consent {
            logout: true,
            submit: false,
        },
        now,
    )
    .unwrap();
    assert_eq!(runner.tick(&mut driver, now), Progress::Running);
    driver.unavailable = true;
    for _ in 0..3 {
        assert_eq!(runner.tick(&mut driver, now), Progress::Running);
    }
    assert_eq!(driver.driver.calls.len(), 1);
    driver.unavailable = false;
    assert_eq!(runner.tick(&mut driver, now), Progress::Running);
    assert_eq!(
        runner.tick(&mut driver, now),
        Progress::Complete(Outcome::LoggedOut)
    );
    let mut runner = Runner::new(
        definition(&value),
        "logout",
        Consent {
            logout: true,
            submit: false,
        },
        now,
    )
    .unwrap();
    driver.driver.page = "home";
    runner.tick(&mut driver, now);
    driver.focus_lost = true;
    assert_eq!(
        runner.tick(&mut driver, now),
        Progress::Stopped(Error::FocusChanged)
    );
    driver.focus_lost = false;
    assert_eq!(
        runner.tick(&mut driver, now),
        Progress::Stopped(Error::FocusChanged)
    );
}

#[test]
fn transitions_dispatch_once_and_accept_a_later_recorded_state() {
    struct TransitionDriver {
        inner: Fake,
        fail: bool,
        transitions: usize,
    }
    impl Driver for TransitionDriver {
        fn guard(&mut self) -> Result<(), Error> {
            self.inner.guard()
        }
        fn observe(&mut self) -> Result<Snapshot, Error> {
            self.inner.observe()
        }
        fn click(&mut self, s: &Snapshot, i: usize) -> Result<(), Error> {
            self.inner.click(s, i)
        }
        fn click_transitioning(
            &mut self,
            s: &Snapshot,
            i: usize,
            timeout: Duration,
        ) -> Result<(), Error> {
            assert!(timeout <= Duration::from_secs(30));
            self.transitions += 1;
            self.inner.click(s, i)?;
            if self.fail && self.transitions == 2 {
                Err(Error::ProcessExited)
            } else {
                Ok(())
            }
        }
        fn fill(&mut self, s: &Snapshot, i: usize, f: CredentialField) -> Result<(), Error> {
            self.inner.fill(s, i, f)
        }
    }
    let mut value = source();
    value["flows"]["logout"]["steps"][1]["next_states"] = json!(["password", "verification"]);
    for fail in [false, true] {
        let now = Instant::now();
        let mut driver = TransitionDriver {
            inner: Fake::new(value.clone()),
            fail,
            transitions: 0,
        };
        let mut runner = Runner::new(
            definition(&value),
            "logout",
            Consent {
                logout: true,
                submit: false,
            },
            now,
        )
        .unwrap();
        for _ in 0..6 {
            runner.tick(&mut driver, now);
        }
        assert_eq!(driver.transitions, 2);
        assert_eq!(driver.inner.calls, ["click:home", "click:menu"]);
        assert_eq!(
            runner.tick(&mut driver, now),
            if fail {
                Progress::Stopped(Error::ProcessExited)
            } else {
                Progress::Complete(Outcome::LoggedOut)
            }
        );
    }
    let mut direct = source();
    direct["flows"]["logout"]["steps"][1]["next_states"] = json!(["password"]);
    let now = Instant::now();
    let mut driver = Fake::new(direct.clone());
    let mut runner = Runner::new(
        definition(&direct),
        "logout",
        Consent {
            logout: true,
            submit: false,
        },
        now,
    )
    .unwrap();
    assert_eq!(runner.tick(&mut driver, now), Progress::Running);
    assert_eq!(runner.tick(&mut driver, now), Progress::Running);
    assert_eq!(
        runner.tick(&mut driver, now),
        Progress::Complete(Outcome::LoggedOut)
    );
}

#[test]
fn schema_v2_is_migrated_to_state_transitions_without_restart_hints() {
    let mut value = source();
    value["schema_version"] = json!(2);
    for flow in value["flows"].as_object_mut().unwrap().values_mut() {
        for step in flow["steps"].as_array_mut().unwrap() {
            let object = step.as_object_mut().unwrap();
            if let Some(next_states) = object.remove("next_states") {
                object.insert("next_state".into(), next_states[0].clone());
                object.insert("expect_restart".into(), json!(true));
            }
        }
    }
    let definition = Definition::parse(&value.to_string()).unwrap();
    let exported = definition.to_json().unwrap();
    assert!(exported.contains("\"schema_version\": 3"));
    assert!(exported.contains("next_states"));
    assert!(!exported.contains("expect_restart"));
}

#[test]
fn draft_recheck_rejects_overlap_and_honors_overlay_exclusions() {
    use autologin_core::switcher::builder::Builder;
    let mut builder = Builder::new(
        "recheck".into(),
        autologin_core::Platform::Macos,
        "org.example.demo".into(),
    )
    .unwrap();
    let home = vec![node(Role::Button, "账号", None, None)];
    let mut menu = home.clone();
    menu.push(node(Role::MenuItem, "退出", None, None));
    builder
        .record_state(
            "home".into(),
            StateKind::LoggedIn,
            home.clone(),
            &[("menu".into(), 0)],
        )
        .unwrap();
    builder
        .record_state(
            "menu".into(),
            StateKind::Intermediate,
            menu.clone(),
            &[("logout".into(), 1)],
        )
        .unwrap();
    assert_eq!(builder.verify_state("menu", &menu), Err(Error::Ambiguous));
    builder.exclude_control("home", "menu", "logout").unwrap();
    assert_eq!(builder.verify_state("menu", &menu), Ok(()));
    assert_eq!(builder.verify_state("home", &home), Ok(()));
    assert_eq!(
        builder.verify_state("home", &menu),
        Err(Error::UnknownState)
    );
    assert_eq!(builder.verify_state("home", &[]), Err(Error::UnknownState));
}

#[test]
fn pointer_method_is_explicit_and_unsupported_drivers_never_fall_back() {
    let mut value = source();
    value["flows"]["logout"]["steps"][0]["method"] = json!("pointer");
    let now = Instant::now();
    let mut runner = Runner::new(
        definition(&value),
        "logout",
        Consent {
            logout: true,
            submit: false,
        },
        now,
    )
    .unwrap();
    let mut driver = Fake::new(value.clone());
    assert_eq!(
        runner.tick(&mut driver, now),
        Progress::Stopped(Error::Driver)
    );
    assert_eq!(
        runner.tick(&mut driver, now),
        Progress::Stopped(Error::Driver)
    );
    assert!(driver.calls.is_empty());
    assert_eq!(driver.page, "home");
    value["flows"]["logout"]["steps"][0]["method"] = json!("automatic_fallback");
    assert!(Definition::parse(&value.to_string()).is_err());
    value["flows"]["logout"]["steps"][0]["method"] = json!("pointer");
    value["flows"]["logout"]["steps"][0]["x"] = json!(100);
    assert!(Definition::parse(&value.to_string()).is_err());
}

#[test]
fn failed_pointer_dispatch_is_never_replayed_or_replaced_with_axpress() {
    struct PointerDriver(Fake);
    impl Driver for PointerDriver {
        fn guard(&mut self) -> Result<(), Error> {
            self.0.guard()
        }
        fn observe(&mut self) -> Result<Snapshot, Error> {
            self.0.observe()
        }
        fn click(&mut self, snapshot: &Snapshot, node: usize) -> Result<(), Error> {
            self.0.click(snapshot, node)
        }
        fn fill(
            &mut self,
            snapshot: &Snapshot,
            node: usize,
            field: CredentialField,
        ) -> Result<(), Error> {
            self.0.fill(snapshot, node, field)
        }
        fn pointer_click(&mut self, _: &Snapshot, _: usize) -> Result<(), Error> {
            self.0.calls.push("pointer".into());
            self.0.page = "menu"; // The input may already have taken effect.
            Err(Error::Timeout)
        }
    }
    let mut value = source();
    value["flows"]["logout"]["steps"][0]["method"] = json!("pointer");
    let now = Instant::now();
    let mut driver = PointerDriver(Fake::new(value.clone()));
    let mut runner = Runner::new(
        definition(&value),
        "logout",
        Consent {
            logout: true,
            submit: false,
        },
        now,
    )
    .unwrap();
    assert_eq!(
        runner.tick(&mut driver, now),
        Progress::Stopped(Error::Timeout)
    );
    assert_eq!(
        runner.tick(&mut driver, now),
        Progress::Stopped(Error::Timeout)
    );
    assert_eq!(driver.0.calls, vec!["pointer"]);
    assert_eq!(runner.attempted_actions, 1);
    // A restart-capable AX driver is not implicitly a restart-capable pointer driver.
    let snapshot = driver.observe().unwrap();
    assert_eq!(
        driver.click_configured(&snapshot, 2, ClickMethod::Pointer, Duration::from_secs(1)),
        Err(Error::Timeout)
    );
    assert_eq!(driver.0.calls, vec!["pointer", "pointer"]);
}

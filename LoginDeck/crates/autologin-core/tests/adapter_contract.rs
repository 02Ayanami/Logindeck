use autologin_core::{adapter::*, Platform};
use serde_json::{json, Value};
fn source() -> Value {
    serde_json::from_str(include_str!("../../../adapters/qq-macos.json")).unwrap()
}
fn definition(value: Value) -> AppDefinition {
    AppDefinition::parse(&value.to_string()).unwrap()
}
fn node(role: Role, name: &str, parent: Option<usize>) -> Node {
    Node {
        role: Some(role),
        names: vec![name.into()],
        identifier: None,
        parent,
    }
}
fn form(value: &Value) -> Vec<Node> {
    let controls = &value["states"][0]["controls"];
    vec![
        node(Role::Window, "", None),
        node(Role::WebArea, "", Some(0)),
        node(
            Role::TextField,
            controls["username"]["names"][0].as_str().unwrap(),
            Some(1),
        ),
        node(
            Role::SecureTextField,
            controls["password"]["names"][0].as_str().unwrap(),
            Some(1),
        ),
        node(
            Role::Button,
            controls["submit"]["names"][0].as_str().unwrap(),
            Some(1),
        ),
    ]
}
#[test]
fn second_application_uses_identical_detector_and_action_plan_with_only_config_changes() {
    let mut fixture = source();
    fixture["id"] = json!("fixture-macos");
    fixture["bundle_id"] = json!("org.example.fixture");
    for (control, label) in [
        ("username", "Email address"),
        ("password", "Account secret"),
        ("submit", "Continue"),
    ] {
        fixture["states"][0]["controls"][control]["names"] = json!([label]);
    }
    let qq = definition(source());
    let app = definition(fixture.clone());
    let nodes = form(&fixture);
    let found = app.detect(&nodes).unwrap();
    assert_eq!(found.state.kind, StateKind::PasswordLogin);
    assert_eq!(found.controls[&Control::Password], 3);
    assert_eq!(
        found.state.actions,
        [
            Action::Fill {
                field: CredentialField::Username
            },
            Action::Fill {
                field: CredentialField::Password
            }
        ]
    );
    assert!(app.supports(Platform::Macos, "org.example.fixture"));
    assert!(matches!(qq.detect(&nodes), Err(AdapterError::UnknownState)));
}
#[test]
fn duplicate_controls_or_simultaneous_states_stop_recognition() {
    let source = source();
    let adapter = definition(source.clone());
    let mut nodes = form(&source);
    nodes.push(node(
        Role::TextField,
        source["states"][0]["controls"]["username"]["names"][0]
            .as_str()
            .unwrap(),
        Some(1),
    ));
    assert!(matches!(
        adapter.detect(&nodes),
        Err(AdapterError::Ambiguous)
    ));
    let mut simultaneous = form(&source);
    simultaneous.push(node(
        Role::Button,
        source["states"][1]["controls"]["marker"]["names"][0]
            .as_str()
            .unwrap(),
        Some(1),
    ));
    assert!(matches!(
        adapter.detect(&simultaneous),
        Err(AdapterError::Ambiguous)
    ));
}
#[test]
fn ordinary_text_fields_and_controls_outside_the_expected_ancestor_do_not_match() {
    let source = source();
    let adapter = definition(source.clone());
    let mut nodes = form(&source);
    nodes[3].role = Some(Role::TextField);
    assert!(matches!(
        adapter.detect(&nodes),
        Err(AdapterError::UnknownState)
    ));
    let mut nodes = form(&source);
    nodes[3].parent = Some(0);
    assert!(matches!(
        adapter.detect(&nodes),
        Err(AdapterError::UnknownState)
    ));
    nodes[3].parent = Some(3);
    assert!(matches!(adapter.detect(&nodes), Err(AdapterError::Invalid)));
}
#[test]
fn manual_state_produces_only_a_user_handoff() {
    let source = source();
    let adapter = definition(source.clone());
    let nodes = vec![
        node(Role::Window, "", None),
        node(Role::WebArea, "", Some(0)),
        node(
            Role::Button,
            source["states"][1]["controls"]["marker"]["names"][0]
                .as_str()
                .unwrap(),
            Some(1),
        ),
    ];
    let found = adapter.detect(&nodes).unwrap();
    assert_eq!(found.state.kind, StateKind::Manual);
    assert_eq!(found.state.actions, [Action::RequestUser]);
}
#[test]
fn configuration_cannot_add_scripts_submit_actions_or_redirect_passwords() {
    for action in [
        json!({"type":"shell", "command":"echo test"}),
        json!({"type":"click", "target":"submit"}),
        json!({"type":"fill", "field":"password", "target":"username"}),
    ] {
        let mut fixture = source();
        fixture["states"][0]["actions"][1] = action;
        assert!(AppDefinition::parse(&fixture.to_string()).is_err());
    }
    let mut fixture = source();
    fixture["states"][0]["controls"]["password"]["role"] = json!("text_field");
    assert!(AppDefinition::parse(&fixture.to_string()).is_err());
    let mut fixture = source();
    fixture["api_key"] = json!("not-allowed");
    assert!(AppDefinition::parse(&fixture.to_string()).is_err());
    let mut fixture = source();
    fixture["schema_version"] = json!(2);
    assert!(matches!(
        AppDefinition::parse(&fixture.to_string()),
        Err(AdapterError::UnsupportedVersion)
    ));
}
#[test]
fn optional_identifier_is_an_additional_constraint_not_an_alternative() {
    let mut fixture = source();
    fixture["states"][0]["controls"]["username"]["identifier"] = json!("login-user");
    let adapter = definition(fixture.clone());
    let mut nodes = form(&fixture);
    assert!(matches!(
        adapter.detect(&nodes),
        Err(AdapterError::UnknownState)
    ));
    nodes[2].identifier = Some("login-user".into());
    assert!(adapter.detect(&nodes).is_ok());
}
#[test]
fn registry_exposes_only_supported_platform_and_application_pairs() {
    assert!(builtin_for_application(Platform::Macos, "com.tencent.qq")
        .unwrap()
        .is_some());
    assert!(builtin_for_application(Platform::Macos, "unknown.app")
        .unwrap()
        .is_none());
}

#[test]
fn saved_selector_resolves_fresh_indexes_and_rejects_duplicates_or_invalid_trees() {
    let original = vec![node(Role::Button, "退出", None)];
    let selector = Selector::from_node(&original, 0).unwrap();
    let mut fresh = vec![
        node(Role::StaticText, "无关内容", None),
        original[0].clone(),
    ];
    assert_eq!(selector.locate(&fresh), Ok(1));
    fresh.push(original[0].clone());
    assert_eq!(selector.locate(&fresh), Err(AdapterError::Ambiguous));
    assert_eq!(selector.locate(&[]), Err(AdapterError::UnknownState));
    fresh[0].parent = Some(0);
    assert_eq!(selector.locate(&fresh), Err(AdapterError::Invalid));
}

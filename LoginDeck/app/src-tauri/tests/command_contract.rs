#[test]
fn registers_exactly_the_desktop_foundation_command_surface() {
    let source = include_str!("../src/lib.rs");
    let expected = [
        "list_websites",
        "next_account_number",
        "edit_website_group",
        "remove_application",
        "save_website",
        "delete_website",
        "copy_website_username",
        "copy_website_password",
        "rescan_applications",
        "get_scan_status",
        "cancel_application_scan",
        "confirm_application_scan",
        "import_applications",
        "list_applications",
        "get_application_icon",
        "get_scan_candidate_icon",
        "save_application_account",
        "delete_application_account",
        "copy_application_username",
        "copy_application_password",
        "launch_application",
        "install_edge_extension",
        "open_edge_extensions",
        "open_edge_extension_folder",
        "get_settings",
        "get_browser_capture_settings",
        "set_browser_capture_enabled",
        "set_locale",
        "get_credential_maintenance",
        "retry_credential_cleanup",
    ];
    let handler = source
        .split("tauri::generate_handler![")
        .nth(1)
        .and_then(|tail| tail.split("]").next())
        .expect("generate_handler invocation");
    let registered = handler
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(registered, expected);
    for forbidden in [
        "verify_and_launch",
        "shell",
        "clipboard",
        "http",
        "fs",
        "fill",
        "switch",
        "login",
    ] {
        assert!(
            !handler.contains(forbidden),
            "unexpected command capability: {forbidden}"
        );
    }
}

#[test]
fn historical_automatic_login_modules_are_not_compiled_by_the_desktop() {
    let root = include_str!("../src/lib.rs");
    let commands = include_str!("../src/commands/mod.rs");
    for source in [root, commands] {
        for forbidden in [
            "mod fill",
            "mod login",
            "mod workflow",
            "platform_runtime::login",
        ] {
            assert!(
                !source.contains(forbidden),
                "compiled automatic login: {forbidden}"
            );
        }
    }
}

#[test]
fn capability_allows_only_the_main_window_and_dialog_opener_operations() {
    let capability: serde_json::Value =
        serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
    assert_eq!(capability["windows"], serde_json::json!(["main"]));
    assert_eq!(
        capability["permissions"],
        serde_json::json!(["core:default", "dialog:allow-open", {"identifier": "opener:allow-open-url", "allow": [{"url": "http://*"}, {"url": "https://*"}]}])
    );
}

#[test]
fn command_responses_do_not_expose_secret_references() {
    // The command modules also have serialization unit tests that construct a real metadata
    // record containing an opaque reference. Keep this lightweight contract focused on the
    // production DTO declarations, excluding those test fixtures.
    let websites = include_str!("../src/commands/websites.rs")
        .split("#[cfg(test)]")
        .next()
        .unwrap();
    let applications = include_str!("../src/commands/applications.rs")
        .split("#[cfg(test)]")
        .next()
        .unwrap();
    assert!(!websites.contains("password_credential_ref"));
    assert!(!applications.contains("password_credential_ref"));
    assert!(!websites.contains("SecretRef"));
    assert!(!applications.contains("SecretRef"));
}

#[test]
fn edge_commands_route_windows_through_the_platform_facade() {
    let source = include_str!("../src/commands/edge.rs");
    assert!(source.contains("platform_runtime::edge::install"));
    assert!(source.contains("platform_runtime::edge::open_extensions"));
    assert!(source.contains("platform_runtime::edge::reveal_extension"));
    assert!(!source.contains("if !cfg!(target_os = \"macos\")"));
}

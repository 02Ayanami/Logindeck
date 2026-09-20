//! Audit the tests selected by the macOS workspace preflight without a macOS SDK.
//! Every new off-Windows test requires an explicit portability review here. This
//! catches missing function, module and integration-crate cfg gates on Windows CI.
use std::{collections::BTreeSet, fs, path::Path};
use syn::{punctuated::Punctuated, Attribute, Expr, Item, Lit, Meta, Token};

fn cfg_matches(meta: &Meta, windows: bool) -> bool {
    match meta {
        Meta::Path(path) if path.is_ident("test") => true,
        Meta::Path(path) if path.is_ident("windows") => windows,
        Meta::Path(path) if path.is_ident("unix") => !windows,
        Meta::NameValue(value) if value.path.is_ident("target_os") => {
            let Expr::Lit(value) = &value.value else {
                panic!("unsupported target_os cfg expression")
            };
            let Lit::Str(value) = &value.lit else {
                panic!("unsupported target_os cfg literal")
            };
            value.value() == if windows { "windows" } else { "macos" }
        }
        Meta::List(list) => {
            let terms = list
                .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                .unwrap();
            if list.path.is_ident("all") {
                terms.iter().all(|term| cfg_matches(term, windows))
            } else if list.path.is_ident("any") {
                terms.iter().any(|term| cfg_matches(term, windows))
            } else if list.path.is_ident("not") && terms.len() == 1 {
                !cfg_matches(&terms[0], windows)
            } else {
                panic!("unsupported cfg operator; extend the boundary audit")
            }
        }
        _ => panic!("unsupported cfg; extend the boundary audit"),
    }
}

fn enabled(attrs: &[Attribute], windows: bool) -> bool {
    let mut selected = true;
    for attr in attrs {
        // Do not silently misinterpret a new conditional-attribute convention.
        assert!(
            !attr.path().is_ident("cfg_attr"),
            "audit cfg_attr explicitly"
        );
        if attr.path().is_ident("cfg") {
            selected &= cfg_matches(&attr.parse_args::<Meta>().unwrap(), windows);
        }
    }
    selected
}

fn inspect_file(file: &Path, prefix: &str, windows: bool, tests: &mut BTreeSet<String>) {
    let parsed = syn::parse_file(&fs::read_to_string(file).unwrap()).unwrap();
    if enabled(&parsed.attrs, windows) {
        let directory = if matches!(file.file_stem().unwrap().to_str(), Some("lib" | "mod")) {
            file.parent().unwrap().to_owned()
        } else {
            file.with_extension("")
        };
        inspect_items(&parsed.items, &directory, prefix, windows, tests);
    }
}

fn inspect_items(
    items: &[Item],
    directory: &Path,
    prefix: &str,
    windows: bool,
    tests: &mut BTreeSet<String>,
) {
    for item in items {
        match item {
            Item::Mod(module) if enabled(&module.attrs, windows) => {
                assert!(
                    !module.attrs.iter().any(|attr| attr.path().is_ident("path")),
                    "audit explicit module paths before adding them"
                );
                let name = module.ident.to_string();
                let prefix = format!("{prefix}::{name}");
                if let Some((_, items)) = &module.content {
                    inspect_items(items, &directory.join(name), &prefix, windows, tests);
                } else {
                    let flat = directory.join(format!("{name}.rs"));
                    let file = if flat.exists() {
                        flat
                    } else {
                        directory.join(name).join("mod.rs")
                    };
                    inspect_file(&file, &prefix, windows, tests);
                }
            }
            Item::Fn(function) if enabled(&function.attrs, windows) => {
                if function.attrs.iter().any(|attr| {
                    attr.path()
                        .segments
                        .last()
                        .is_some_and(|segment| segment.ident == "test")
                }) {
                    tests.insert(format!("{prefix}::{}", function.sig.ident));
                }
                // The custom launch harness has no #[test] and Cargo runs it on
                // macOS too. Its selected non-Windows entry must remain inert.
                if !windows && prefix == "launch_native" && function.sig.ident == "main" {
                    assert!(
                        function.block.stmts.is_empty(),
                        "non-Windows launch harness must be inert"
                    );
                    tests.insert("launch_native::main (inert)".into());
                }
            }
            _ => {}
        }
    }
}

fn selected_tests(windows: bool) -> BTreeSet<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut tests = BTreeSet::new();
    inspect_file(&root.join("src/lib.rs"), "lib", windows, &mut tests);
    for entry in fs::read_dir(root.join("tests")).unwrap() {
        let file = entry.unwrap().path();
        if file.extension().is_some_and(|extension| extension == "rs") {
            inspect_file(
                &file,
                file.file_stem().unwrap().to_str().unwrap(),
                windows,
                &mut tests,
            );
        }
    }
    tests
}

#[test]
fn macos_workspace_selects_only_reviewed_portable_tests() {
    // These exercise AUMIDs, bounded text/UTF-16 parsing, string-based Windows
    // syntax matching, rejection before path resolution, and sanitized failures.
    // They never need Windows absolute/parent/file-name semantics or native APIs.
    let portable: BTreeSet<String> = [
        "app_catalog::package::tests::application_record_budget_is_applied_before_filtering",
        "app_catalog::package::tests::executable_overlap_requires_exact_aumid_and_package_owned_path",
        "app_catalog::package::tests::malformed_and_oversized_identity_fields_are_skipped",
        "app_catalog::package::tests::package_fixtures_keep_only_current_launchable_application_identities",
        "app_catalog::package::tests::presentation_failure_degrades_and_signature_tracks_registration_not_display_name",
        "app_catalog::shortcut::tests::raw_shortcuts_never_parse_arguments_or_accept_unsafe_targets",
        "app_catalog::shortcut::tests::shortcut_buffers_require_a_terminator_and_valid_utf16",
        "app_catalog::shortcut::tests::shortcut_match_requires_install_anchor_and_name_with_stable_order",
        "app_catalog::shortcut::tests::shortcut_valid_text_with_last_slot_terminator_is_rejected_as_saturated",
        "app_catalog::tests::blocking_worker_failure_is_sanitized",
        "app_catalog::tests::discovery_failure_semantics_preserve_successful_empty_sources_and_sanitize",
        "app_catalog::win32::tests::bounds_and_helpers_fail_closed",
        "app_catalog::win32::tests::canonical_aliases_do_not_bypass_helper_or_command_host_filters",
        "app_catalog::win32::tests::component_metadata_expansion_failure_or_overflow_rejects_entry",
        "app_catalog::win32::tests::display_icon_is_a_path_and_optional_index_never_a_command",
        "app_catalog::win32::tests::raw_registry_budget_counts_malformed_records_before_normalization",
        "app_catalog::win32::tests::registry_utf16_decode_is_typed_bounded_and_strict",
        "app_catalog::win32::tests::release_type_expandable_update_is_excluded",
        "app_catalog::win32::tests::runtime_family_filters_preserve_named_user_tools",
        "app_catalog::win32::tests::source_failure_is_sanitized_and_other_scopes_are_still_requested",
        "error::tests::native_status_is_recorded_without_error_message_params",
        "error::tests::native_statuses_map_to_stable_credential_errors",
        "target::tests::aumid_round_trips_without_guessing",
        "target::tests::encode_rejects_directly_constructed_invalid_variants",
        "target::tests::target_rejects_every_invalid_encoding_class",
    ]
    .into_iter()
    .map(|name| format!("lib::{name}"))
    .chain([
        "launch_native::main (inert)".into(),
        "test_boundaries::macos_workspace_selects_only_reviewed_portable_tests".into(),
    ])
    .collect();
    let macos = selected_tests(false);
    let windows = selected_tests(true);
    let unexpected: Vec<_> = macos.difference(&portable).collect();
    let missing: Vec<_> = portable.difference(&macos).collect();
    assert!(
        unexpected.is_empty() && missing.is_empty(),
        "Review portability or add cfg(windows). Unexpected macOS tests: {unexpected:#?}; missing portable coverage: {missing:#?}"
    );
    assert!(portable
        .iter()
        .filter(|name| !name.contains(" (inert)"))
        .all(|name| windows.contains(name)));
    eprintln!(
        "cfg audit: {} Windows tests; {} macOS tests plus inert launch harness",
        windows.len(),
        macos.len() - 1
    );
}

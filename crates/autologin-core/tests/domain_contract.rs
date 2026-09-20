use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use autologin_core::{
    AppError, ApplicationCatalog, ApplicationRecord, Clipboard, DiscoveredApplication, Locale,
    Platform, VerifiedApplication,
};
use secrecy::SecretString;

/// A tiny executor keeps this contract test dependency-free while still exercising the async
/// interface exactly as a platform implementation will use it.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    fn no_op(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(
            std::ptr::null(),
            &RawWakerVTable::new(clone, no_op, no_op, no_op),
        )
    }

    let waker = unsafe { Waker::from_raw(clone(std::ptr::null())) };
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("contract test future unexpectedly yielded"),
    }
}

#[test]
fn locale_defaults_to_english() {
    assert_eq!(Locale::default(), Locale::En);
}

#[test]
fn locale_serializes_with_stable_wire_names() {
    assert_eq!(serde_json::to_string(&Locale::En).unwrap(), "\"en\"");
    assert_eq!(serde_json::to_string(&Locale::ZhCn).unwrap(), "\"zh-CN\"");
}

#[test]
fn windows_platform_serializes_with_a_stable_wire_name() {
    assert_eq!(
        serde_json::to_string(&Platform::Windows).unwrap(),
        "\"windows\""
    );
    assert_eq!(
        serde_json::from_str::<Platform>("\"windows\"").unwrap(),
        Platform::Windows
    );
}

#[test]
fn application_identity_is_platform_neutral() {
    let app = ApplicationRecord::new(
        Platform::Macos,
        "com.valvesoftware.steam".into(),
        "Steam".into(),
    )
    .unwrap();

    assert_eq!(app.platform_application_id, "com.valvesoftware.steam");
}

#[test]
fn blank_application_identity_is_rejected() {
    assert!(ApplicationRecord::new(Platform::Macos, " ".into(), "Steam".into()).is_err());
}

#[test]
fn invalid_secret_reference_json_is_rejected() {
    let error = serde_json::from_str::<autologin_core::SecretRef>(r#"" ""#);

    assert!(error.is_err());
}

#[test]
fn invalid_application_identity_json_is_rejected() {
    let application = ApplicationRecord::new(
        Platform::Macos,
        "com.valvesoftware.steam".into(),
        "Steam".into(),
    )
    .unwrap();
    let mut value = serde_json::to_value(application).unwrap();
    value["launch_target"] = serde_json::json!("/Applications/Steam.app");
    value["platform_application_id"] = serde_json::json!(" ");

    assert!(serde_json::from_value::<ApplicationRecord>(value).is_err());
}

#[test]
fn invalid_application_display_name_json_is_rejected() {
    let application = ApplicationRecord::new(
        Platform::Macos,
        "com.valvesoftware.steam".into(),
        "Steam".into(),
    )
    .unwrap();
    let mut value = serde_json::to_value(application).unwrap();
    value["launch_target"] = serde_json::json!("/Applications/Steam.app");
    value["display_name"] = serde_json::json!(" ");

    assert!(serde_json::from_value::<ApplicationRecord>(value).is_err());
}

#[test]
fn empty_application_launch_target_json_is_rejected() {
    let application = ApplicationRecord::new(
        Platform::Macos,
        "com.valvesoftware.steam".into(),
        "Steam".into(),
    )
    .unwrap();
    let value = serde_json::to_value(application).unwrap();

    assert!(serde_json::from_value::<ApplicationRecord>(value).is_err());
}

#[test]
fn application_path_limit_counts_utf8_bytes() {
    assert!(autologin_core::validate_application_metadata(
        "Steam",
        "com.valvesoftware.steam",
        &"a".repeat(4096),
        &[],
        None,
        "",
        None,
    )
    .is_ok());
    assert!(autologin_core::validate_application_metadata(
        "Steam",
        "com.valvesoftware.steam",
        &"界".repeat(1366),
        &[],
        None,
        "",
        None,
    )
    .is_err());
}

#[test]
fn validation_errors_keep_stable_codes_and_structured_parameters() {
    let mut params = BTreeMap::new();
    params.insert("field".to_owned(), "platform_application_id".to_owned());
    let error = AppError::new("validation.invalid_field").with_params(params);

    assert_eq!(error.code(), "validation.invalid_field");
    assert_eq!(
        error.params.get("field").map(String::as_str),
        Some("platform_application_id")
    );
}

#[test]
fn secret_reference_debug_output_is_opaque() {
    let reference = autologin_core::SecretRef::new("keychain-account").unwrap();

    assert_eq!(format!("{reference:?}"), "SecretRef(<opaque>)");
    assert!(!format!("{reference:?}").contains("keychain-account"));
}

#[derive(Clone, Default)]
struct ContractClipboard {
    next_token: Arc<Mutex<u64>>,
    latest_token: Arc<Mutex<Option<u64>>>,
}

#[async_trait::async_trait]
impl Clipboard for ContractClipboard {
    type ChangeToken = u64;

    async fn write(&self, _value: SecretString) -> Result<Self::ChangeToken, AppError> {
        let mut next_token = self.next_token.lock().unwrap();
        *next_token += 1;
        let token = *next_token;
        *self.latest_token.lock().unwrap() = Some(token);
        Ok(token)
    }

    async fn clear_if_unchanged(&self, token: &Self::ChangeToken) -> Result<(), AppError> {
        let mut latest_token = self.latest_token.lock().unwrap();
        if latest_token.as_ref() == Some(token) {
            *latest_token = None;
        }
        Ok(())
    }
}

#[test]
fn clipboard_contract_returns_a_change_token_for_conditional_clear() {
    let clipboard = ContractClipboard::default();
    let first = block_on(clipboard.write(SecretString::from("first"))).unwrap();
    let second = block_on(clipboard.write(SecretString::from("second"))).unwrap();

    block_on(clipboard.clear_if_unchanged(&first)).unwrap();
    assert_eq!(*clipboard.latest_token.lock().unwrap(), Some(second));

    block_on(clipboard.clear_if_unchanged(&second)).unwrap();
    assert_eq!(*clipboard.latest_token.lock().unwrap(), None);
}

#[derive(Default)]
struct ContractCatalog;

#[async_trait::async_trait]
impl ApplicationCatalog for ContractCatalog {
    async fn discover(&self) -> Result<Vec<DiscoveredApplication>, AppError> {
        Ok(Vec::new())
    }

    async fn import_bundle(
        &self,
        _path: &std::path::Path,
    ) -> Result<Vec<DiscoveredApplication>, AppError> {
        Ok(Vec::new())
    }

    async fn verify_and_launch(
        &self,
        app: &ApplicationRecord,
    ) -> Result<VerifiedApplication, AppError> {
        Ok(VerifiedApplication {
            application: app.clone(),
            verified_launch_target: app.launch_target.clone(),
            launched_process_id: 42,
        })
    }
}

#[test]
fn application_catalog_verifies_and_launches_atomically() {
    let app = ApplicationRecord::new(
        Platform::Macos,
        "com.valvesoftware.steam".into(),
        "Steam".into(),
    )
    .unwrap();

    let verified = block_on(ContractCatalog.verify_and_launch(&app)).unwrap();
    assert_eq!(verified.application.id, app.id);
    assert_eq!(verified.launched_process_id, 42);
}

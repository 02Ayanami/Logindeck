#![cfg(target_os = "windows")]

use autologin_core::{CredentialStore, SecretRef};
use platform_windows::{credential_last_status, record_last_status, WindowsCredentialStore};
use secrecy::{ExposeSecret, SecretString};
use std::sync::{Arc, Barrier, Mutex};
use windows::{
    core::PCWSTR,
    Win32::Security::Credentials::{CredDeleteW, CRED_MAX_CREDENTIAL_BLOB_SIZE, CRED_TYPE_GENERIC},
};

static TEST_LOCK: Mutex<()> = Mutex::new(());

struct CredentialCleanup {
    target: Vec<u16>,
}

impl CredentialCleanup {
    fn new(service: &str, reference: &SecretRef) -> Self {
        Self {
            target: format!("LoginDeck/{service}/{}", reference.as_str())
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect(),
        }
    }
}

impl Drop for CredentialCleanup {
    fn drop(&mut self) {
        // The unique service and reference keep panic cleanup scoped to this test's one item.
        let _ = unsafe { CredDeleteW(PCWSTR(self.target.as_ptr()), CRED_TYPE_GENERIC, None) };
    }
}

fn unique_target() -> (String, SecretRef, CredentialCleanup) {
    let service = format!("LoginDeck.Tests.{}", uuid::Uuid::new_v4());
    let reference = SecretRef::new(format!("credential-manager:v1:{}", uuid::Uuid::new_v4()))
        .expect("unique test reference must be valid");
    let cleanup = CredentialCleanup::new(&service, &reference);
    (service, reference, cleanup)
}

fn fixture() -> (WindowsCredentialStore, SecretRef, CredentialCleanup) {
    let (service, reference, cleanup) = unique_target();
    (WindowsCredentialStore::new(service), reference, cleanup)
}

#[tokio::test]
async fn generic_credential_round_trip_and_delete_are_real() {
    let _serial = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let (store, reference, _cleanup) = fixture();

    store
        .put(&reference, "fixture", SecretString::from("correct horse"))
        .await
        .unwrap();
    assert!(
        store
            .reveal(&reference, "test")
            .await
            .unwrap()
            .expose_secret()
            == "correct horse",
        "revealed credential did not match"
    );
    store.delete(&reference).await.unwrap();
    assert_eq!(
        store
            .reveal(&reference, "missing")
            .await
            .unwrap_err()
            .code(),
        "credential.not_found"
    );
}

#[tokio::test]
async fn put_never_overwrites_an_existing_credential() {
    let _serial = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let (store, reference, _cleanup) = fixture();

    store
        .put(&reference, "fixture", SecretString::from("first"))
        .await
        .unwrap();
    let error = store
        .put(&reference, "fixture", SecretString::from("second"))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "storage.conflict");
    assert!(
        store
            .reveal(&reference, "verify no overwrite")
            .await
            .unwrap()
            .expose_secret()
            == "first",
        "existing credential was overwritten"
    );
}

#[tokio::test]
async fn maximum_utf8_blob_size_succeeds() {
    let _serial = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let (store, reference, _cleanup) = fixture();
    let value = "x".repeat(CRED_MAX_CREDENTIAL_BLOB_SIZE as usize);

    store
        .put(&reference, "fixture", SecretString::from(value.clone()))
        .await
        .unwrap();
    assert!(
        store
            .reveal(&reference, "verify maximum")
            .await
            .unwrap()
            .expose_secret()
            == value,
        "maximum-size credential did not round trip"
    );
}

#[tokio::test]
async fn oversized_utf8_blob_is_rejected_without_creating_an_item() {
    let _serial = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let (store, reference, _cleanup) = fixture();
    let value = "x".repeat(CRED_MAX_CREDENTIAL_BLOB_SIZE as usize + 1);

    let error = store
        .put(&reference, "fixture", SecretString::from(value))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "validation.invalid_field");
    assert_eq!(error.params.get("field"), Some(&"password".to_owned()));
    assert_eq!(
        store
            .reveal(&reference, "prove oversized write created nothing")
            .await
            .unwrap_err()
            .code(),
        "credential.not_found"
    );
}

#[tokio::test]
async fn missing_reveal_and_delete_have_stable_errors_and_native_status() {
    let _serial = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let (store, reference, _cleanup) = fixture();

    assert_eq!(
        store
            .reveal(&reference, "missing reveal")
            .await
            .unwrap_err()
            .code(),
        "credential.not_found"
    );
    assert_eq!(credential_last_status(), Some(1_168));
    assert_eq!(
        store.delete(&reference).await.unwrap_err().code(),
        "credential.not_found"
    );
    assert_eq!(credential_last_status(), Some(1_168));
}

#[test]
fn concurrent_puts_across_store_instances_have_one_winner() {
    let _serial = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());

    for _ in 0..64 {
        let (service, reference, _cleanup) = unique_target();
        let barrier = Arc::new(Barrier::new(3));
        let writer = |secret: &'static str| {
            let store = WindowsCredentialStore::new(service.clone());
            let reference = reference.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("writer runtime must build");
                barrier.wait();
                runtime.block_on(store.put(&reference, "fixture", SecretString::from(secret)))
            })
        };

        let first = writer("concurrent first");
        let second = writer("concurrent second");
        barrier.wait();
        let first = first.join().expect("first writer must not panic");
        let second = second.join().expect("second writer must not panic");

        assert_eq!(
            usize::from(first.is_ok()) + usize::from(second.is_ok()),
            1,
            "exactly one concurrent writer must succeed"
        );
        let winner = if first.is_ok() {
            assert_eq!(second.unwrap_err().code(), "storage.conflict");
            "concurrent first"
        } else {
            assert_eq!(first.unwrap_err().code(), "storage.conflict");
            "concurrent second"
        };

        let store = WindowsCredentialStore::new(service);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("verification runtime must build");
        let revealed = runtime
            .block_on(store.reveal(&reference, "verify concurrent winner"))
            .unwrap();
        assert!(
            revealed.expose_secret() == winner,
            "stored credential did not belong to the successful writer"
        );
        runtime.block_on(store.delete(&reference)).unwrap();
    }
}

#[tokio::test]
async fn expected_absence_during_put_does_not_replace_last_failing_status() {
    let _serial = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let (store, reference, _cleanup) = fixture();
    record_last_status(5);

    store
        .put(&reference, "fixture", SecretString::from("status sequence"))
        .await
        .unwrap();
    assert_eq!(credential_last_status(), Some(5));
    store.delete(&reference).await.unwrap();
    assert_eq!(credential_last_status(), Some(5));

    assert_eq!(
        store
            .reveal(&reference, "record actual missing failure")
            .await
            .unwrap_err()
            .code(),
        "credential.not_found"
    );
    assert_eq!(credential_last_status(), Some(1_168));
}

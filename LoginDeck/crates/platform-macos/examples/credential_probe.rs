//! Explicit local fixture probe. Does not read existing items or print secret material.
use autologin_core::CredentialStore;
use platform_macos::MacCredentialStore;
use secrecy::SecretString;
fn main() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let store = MacCredentialStore::new("com.autologin.desktop.blackbox".into());
            let reference = store.new_reference().unwrap();
            let saved = store
                .put(
                    &reference,
                    "LoginDeck disposable test",
                    SecretString::from("Blackbox-Fixture-2026!"),
                )
                .await;
            println!(
                "Fixture save: {}",
                saved.as_ref().map(|_| "ok").unwrap_or_else(|e| e.code())
            );
            let deleted = store.delete(&reference).await;
            println!(
                "Fixture cleanup: {}",
                deleted.as_ref().map(|_| "ok").unwrap_or_else(|e| e.code())
            );
        });
}

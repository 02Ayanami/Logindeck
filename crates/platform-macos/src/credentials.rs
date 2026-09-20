use std::sync::Arc;

use async_trait::async_trait;
use autologin_core::{AppError, CredentialStore, SecretRef};
use secrecy::SecretString;

static LAST_STATUS: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
pub fn keychain_last_status() -> Option<i32> {
    let status = LAST_STATUS.load(std::sync::atomic::Ordering::Relaxed);
    (status != 0).then_some(status)
}

const CREDENTIAL_NOT_FOUND: &str = "credential.not_found";

/// Narrow test seam around the system Keychain.
///
/// The interface deliberately accepts and returns `SecretString`; implementations must not log
/// the values they receive. Production storage is [`MacKeychainBackend`], while tests can inject
/// an in-memory implementation without creating a Keychain item or prompting the user.
#[async_trait]
trait KeychainBackend: Send + Sync {
    async fn put(
        &self,
        reference: &SecretRef,
        label: &str,
        secret: SecretString,
    ) -> Result<(), AppError>;
    async fn read(&self, reference: &SecretRef, reason: &str) -> Result<SecretString, AppError>;
    async fn delete(&self, reference: &SecretRef) -> Result<(), AppError>;
}

/// Stores each secret as a generic-password Keychain item behind an opaque random account ID.
///
/// No `Debug` implementation is provided because this type owns a backend that can expose
/// secrets. New items use the login Keychain ACL; old unprefixed references retain their
/// Data Protection Keychain route.
pub struct MacCredentialStore {
    keychain: Arc<dyn KeychainBackend>,
}

impl MacCredentialStore {
    /// Uses the macOS Keychain generic-password service supplied by the caller.
    pub fn new(service: String) -> Self {
        Self {
            keychain: Arc::new(MacKeychainBackend::new(service)),
        }
    }

    #[cfg(test)]
    fn with_backend<K>(keychain: K) -> Self
    where
        K: KeychainBackend + 'static,
    {
        Self {
            keychain: Arc::new(keychain),
        }
    }
}

#[async_trait]
impl CredentialStore for MacCredentialStore {
    fn new_reference(&self) -> Result<SecretRef, AppError> {
        SecretRef::new(format!("login-keychain:v1:{}", uuid::Uuid::new_v4()))
    }
    async fn put(
        &self,
        reference: &SecretRef,
        label: &str,
        secret: SecretString,
    ) -> Result<(), AppError> {
        self.keychain.put(reference, label, secret).await
    }

    async fn reveal(&self, reference: &SecretRef, reason: &str) -> Result<SecretString, AppError> {
        if reason.trim().is_empty() {
            return Err(AppError::invalid_field("authentication_reason"));
        }

        // The system Keychain enforces the item ACL. New items have an empty trusted-app list;
        // legacy Data Protection items retain their user-presence access control.
        self.keychain.read(reference, reason).await
    }

    async fn delete(&self, reference: &SecretRef) -> Result<(), AppError> {
        self.keychain.delete(reference).await
    }
}

struct MacKeychainBackend {
    service: String,
}

impl MacKeychainBackend {
    fn new(service: String) -> Self {
        Self { service }
    }
}

#[cfg(target_os = "macos")]
mod native {
    use std::ptr;

    use autologin_core::{AppError, SecretRef};
    use core_foundation::{
        base::{CFOptionFlags, CFType, TCFType},
        data::CFData,
        dictionary::CFMutableDictionary,
        string::CFString,
    };
    use secrecy::{ExposeSecret, SecretString};

    use super::{KeychainBackend, MacKeychainBackend, CREDENTIAL_NOT_FOUND};

    type OsStatus = i32;
    type SecAccessControlRef = *const std::ffi::c_void;
    type CFErrorRef = *const std::ffi::c_void;

    const ERR_SEC_ITEM_NOT_FOUND: OsStatus = -25_300;
    const ERR_SEC_USER_CANCELED: OsStatus = -128;
    const ERR_SEC_AUTH_FAILED: OsStatus = -25_293;
    const ERR_SEC_INTERACTION_NOT_ALLOWED: OsStatus = -25_308;
    const ERR_SEC_INTERACTION_REQUIRED: OsStatus = -25_315;
    const K_SEC_ACCESS_CONTROL_USER_PRESENCE: CFOptionFlags = 1usize;

    #[link(name = "Security", kind = "framework")]
    extern "C" {
        static kSecClass: core_foundation::string::CFStringRef;
        static kSecClassGenericPassword: core_foundation::string::CFStringRef;
        static kSecAttrService: core_foundation::string::CFStringRef;
        static kSecAttrAccount: core_foundation::string::CFStringRef;
        static kSecAttrLabel: core_foundation::string::CFStringRef;
        static kSecACLAuthorizationDecrypt: core_foundation::string::CFStringRef;
        fn SecAccessCopyMatchingACLList(
            access: *const std::ffi::c_void,
            authorization: *const std::ffi::c_void,
        ) -> core_foundation::array::CFArrayRef;
        fn SecACLSetContents(
            acl: *const std::ffi::c_void,
            applications: core_foundation::array::CFArrayRef,
            description: core_foundation::string::CFStringRef,
            selector: u16,
        ) -> OsStatus;
        static kSecAttrAccess: core_foundation::string::CFStringRef;
        fn SecAccessCreate(
            descriptor: core_foundation::string::CFStringRef,
            trusted: core_foundation::array::CFArrayRef,
            access: *mut *const std::ffi::c_void,
        ) -> OsStatus;
        static kSecAttrAccessControl: core_foundation::string::CFStringRef;
        static kSecAttrAccessibleWhenUnlockedThisDeviceOnly: core_foundation::string::CFStringRef;
        static kSecValueData: core_foundation::string::CFStringRef;
        static kSecReturnData: core_foundation::string::CFStringRef;
        static kSecMatchLimit: core_foundation::string::CFStringRef;
        static kSecMatchLimitOne: core_foundation::string::CFStringRef;
        static kSecUseOperationPrompt: core_foundation::string::CFStringRef;
        static kSecUseDataProtectionKeychain: core_foundation::string::CFStringRef;

        fn SecAccessControlCreateWithFlags(
            allocator: *const std::ffi::c_void,
            protection: *const std::ffi::c_void,
            flags: CFOptionFlags,
            error: *mut CFErrorRef,
        ) -> SecAccessControlRef;
        fn SecItemAdd(
            query: core_foundation::dictionary::CFDictionaryRef,
            result: *mut core_foundation::base::CFTypeRef,
        ) -> OsStatus;
        fn SecItemCopyMatching(
            query: core_foundation::dictionary::CFDictionaryRef,
            result: *mut core_foundation::base::CFTypeRef,
        ) -> OsStatus;
        fn SecItemDelete(query: core_foundation::dictionary::CFDictionaryRef) -> OsStatus;
    }

    fn status_error(status: OsStatus) -> AppError {
        // Numeric status only: never print the query, account, label, or secret.
        super::LAST_STATUS.store(status, std::sync::atomic::Ordering::Relaxed);
        eprintln!("Keychain operation failed (OSStatus {status})");
        match status {
            -34_018 => AppError::new("credential.missing_entitlement"),
            ERR_SEC_ITEM_NOT_FOUND => AppError::new(CREDENTIAL_NOT_FOUND),
            ERR_SEC_USER_CANCELED => AppError::new("credential.cancelled"),
            ERR_SEC_AUTH_FAILED => AppError::credential_denied(),
            ERR_SEC_INTERACTION_NOT_ALLOWED | ERR_SEC_INTERACTION_REQUIRED => {
                AppError::new("credential.unavailable")
            }
            _ => AppError::new("credential.unavailable"),
        }
    }

    fn sec_string(value: core_foundation::string::CFStringRef) -> CFString {
        // Security framework constants are process-lifetime CFStrings. Retaining them makes
        // their ownership explicit to the Core Foundation wrapper.
        unsafe { CFString::wrap_under_get_rule(value) }
    }

    fn base_query(service: &str, reference: &SecretRef) -> CFMutableDictionary<CFType, CFType> {
        let class = unsafe { sec_string(kSecClass) };
        let generic_password = unsafe { sec_string(kSecClassGenericPassword) };
        let service_key = unsafe { sec_string(kSecAttrService) };
        let account_key = unsafe { sec_string(kSecAttrAccount) };
        let data_protection_keychain_key = unsafe { sec_string(kSecUseDataProtectionKeychain) };
        let service = CFString::new(service);
        let account = CFString::new(reference.as_str());

        CFMutableDictionary::from_CFType_pairs(&[
            (class.as_CFType(), generic_password.as_CFType()),
            (service_key.as_CFType(), service.as_CFType()),
            (account_key.as_CFType(), account.as_CFType()),
            (
                data_protection_keychain_key.as_CFType(),
                core_foundation::boolean::CFBoolean::from(
                    !reference.as_str().starts_with("login-keychain:v1:"),
                )
                .as_CFType(),
            ),
        ])
    }

    fn join_error() -> AppError {
        AppError::new("credential.unavailable")
    }

    async fn run_security_operation<T, F>(operation: F) -> Result<T, AppError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, AppError> + Send + 'static,
    {
        tokio::task::spawn_blocking(operation)
            .await
            .map_err(|_| join_error())?
    }

    fn add_item(
        service: &str,
        reference: &SecretRef,
        label: &str,
        secret: SecretString,
    ) -> Result<(), AppError> {
        let mut query = base_query(service, reference);
        let label_key = unsafe { sec_string(kSecAttrLabel) };
        let value_key = unsafe { sec_string(kSecValueData) };
        let label = CFString::new(label);
        let secret_data = CFData::from_buffer(secret.expose_secret().as_bytes());
        query.add(&label_key.as_CFType(), &label.as_CFType());
        query.add(&value_key.as_CFType(), &secret_data.as_CFType());
        if reference.as_str().starts_with("login-keychain:v1:") {
            // An empty list (not NULL) means no application may read without confirmation.
            // Never use an allow-all ACL, and leave macOS in control of authorization.
            let trusted = core_foundation::array::CFArray::<CFType>::from_CFTypes(&[]);
            let mut access = ptr::null();
            let status = unsafe {
                SecAccessCreate(
                    label.as_concrete_TypeRef(),
                    trusted.as_concrete_TypeRef(),
                    &mut access,
                )
            };
            if status != 0 {
                return Err(status_error(status));
            }
            if access.is_null() {
                return Err(AppError::new("credential.unavailable"));
            }
            let access = unsafe { CFType::wrap_under_create_rule(access) };
            let acls = unsafe {
                SecAccessCopyMatchingACLList(
                    access.as_CFTypeRef(),
                    kSecACLAuthorizationDecrypt.cast(),
                )
            };
            if acls.is_null() {
                return Err(AppError::new("credential.unavailable"));
            }
            let acls =
                unsafe { core_foundation::array::CFArray::<CFType>::wrap_under_create_rule(acls) };
            if acls.is_empty() {
                return Err(AppError::new("credential.unavailable"));
            }
            for acl in acls.iter() {
                // Require the login Keychain passphrase, even if the creating process is cached.
                let status = unsafe {
                    SecACLSetContents(
                        acl.as_CFTypeRef(),
                        trusted.as_concrete_TypeRef(),
                        label.as_concrete_TypeRef(),
                        1,
                    )
                };
                if status != 0 {
                    return Err(status_error(status));
                }
            }

            query.add(&unsafe { sec_string(kSecAttrAccess) }.as_CFType(), &access);
        } else {
            let access_key = unsafe { sec_string(kSecAttrAccessControl) };
            let accessible = unsafe { sec_string(kSecAttrAccessibleWhenUnlockedThisDeviceOnly) };
            let mut error: CFErrorRef = ptr::null();
            let access_control = unsafe {
                SecAccessControlCreateWithFlags(
                    ptr::null(),
                    accessible.as_CFTypeRef(),
                    K_SEC_ACCESS_CONTROL_USER_PRESENCE,
                    &mut error,
                )
            };
            if !error.is_null() {
                unsafe { core_foundation::base::CFRelease(error) };
            }
            if access_control.is_null() {
                return Err(AppError::new("credential.unavailable"));
            }
            let access_control = unsafe { CFType::wrap_under_create_rule(access_control) };
            query.add(&access_key.as_CFType(), &access_control);
        }

        let status = unsafe { SecItemAdd(query.as_concrete_TypeRef(), ptr::null_mut()) };
        if status == 0 {
            Ok(())
        } else {
            Err(status_error(status))
        }
    }

    fn copy_item(
        service: &str,
        reference: &SecretRef,
        reason: &str,
    ) -> Result<SecretString, AppError> {
        let mut query = base_query(service, reference);
        let return_data_key = unsafe { sec_string(kSecReturnData) };
        let match_limit_key = unsafe { sec_string(kSecMatchLimit) };
        let match_limit_one = unsafe { sec_string(kSecMatchLimitOne) };
        let prompt_key = unsafe { sec_string(kSecUseOperationPrompt) };
        let prompt = CFString::new(reason);
        query.add(
            &return_data_key.as_CFType(),
            &core_foundation::boolean::CFBoolean::true_value().as_CFType(),
        );
        query.add(&match_limit_key.as_CFType(), &match_limit_one.as_CFType());
        query.add(&prompt_key.as_CFType(), &prompt.as_CFType());

        let mut result = ptr::null();
        let status = unsafe { SecItemCopyMatching(query.as_concrete_TypeRef(), &mut result) };
        if status != 0 {
            return Err(status_error(status));
        }

        let data = unsafe { CFType::wrap_under_create_rule(result) }
            .downcast_into::<CFData>()
            .ok_or_else(|| AppError::new("credential.unavailable"))?;
        let value = std::str::from_utf8(data.bytes())
            .map_err(|_| AppError::new("credential.unavailable"))?;
        Ok(SecretString::from(value.to_owned()))
    }

    fn delete_item(service: &str, reference: &SecretRef) -> Result<(), AppError> {
        let query = base_query(service, reference);
        let status = unsafe { SecItemDelete(query.as_concrete_TypeRef()) };
        match status {
            0 | ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            _ => Err(status_error(status)),
        }
    }

    #[async_trait::async_trait]
    impl KeychainBackend for MacKeychainBackend {
        async fn put(
            &self,
            reference: &SecretRef,
            label: &str,
            secret: SecretString,
        ) -> Result<(), AppError> {
            let service = self.service.clone();
            let reference = reference.clone();
            let label = label.to_owned();
            run_security_operation(move || add_item(&service, &reference, &label, secret)).await
        }

        async fn read(
            &self,
            reference: &SecretRef,
            reason: &str,
        ) -> Result<SecretString, AppError> {
            let service = self.service.clone();
            let reference = reference.clone();
            let reason = reason.to_owned();
            run_security_operation(move || copy_item(&service, &reference, &reason)).await
        }

        async fn delete(&self, reference: &SecretRef) -> Result<(), AppError> {
            let service = self.service.clone();
            let reference = reference.clone();
            run_security_operation(move || delete_item(&service, &reference)).await
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{
            base_query, kSecUseDataProtectionKeychain, run_security_operation, sec_string,
            status_error, ERR_SEC_AUTH_FAILED, ERR_SEC_INTERACTION_NOT_ALLOWED,
            ERR_SEC_INTERACTION_REQUIRED, ERR_SEC_ITEM_NOT_FOUND, ERR_SEC_USER_CANCELED,
            K_SEC_ACCESS_CONTROL_USER_PRESENCE,
        };
        use autologin_core::SecretRef;
        use core_foundation::{
            base::{CFOptionFlags, TCFType},
            boolean::CFBoolean,
        };

        #[test]
        #[ignore = "creates one disposable Keychain fixture; run explicitly outside sandbox"]
        fn login_keychain_read_fails_without_user_interaction() {
            use super::*;
            use autologin_core::CredentialStore;
            extern "C" {
                fn SecKeychainSetUserInteractionAllowed(allowed: u8) -> i32;
            }
            struct Restore;
            impl Drop for Restore {
                fn drop(&mut self) {
                    unsafe {
                        SecKeychainSetUserInteractionAllowed(1);
                    }
                }
            }
            let store =
                super::super::MacCredentialStore::new("com.autologin.desktop.blackbox".into());
            let reference = store.new_reference().unwrap();
            let service = "com.autologin.desktop.blackbox";
            add_item(
                service,
                &reference,
                "LoginDeck disposable authorization test",
                SecretString::from("Blackbox-Fixture-2026!"),
            )
            .unwrap();
            assert_eq!(unsafe { SecKeychainSetUserInteractionAllowed(0) }, 0);
            let restore = Restore;
            let read = copy_item(service, &reference, "Test authorization");
            drop(restore);
            delete_item(service, &reference).unwrap();
            assert!(
                read.is_err(),
                "password must not be returned without system authorization"
            );
        }

        #[test]
        fn keychain_statuses_have_stable_non_sensitive_codes() {
            assert_eq!(
                status_error(ERR_SEC_ITEM_NOT_FOUND).code(),
                "credential.not_found"
            );
            assert_eq!(
                status_error(ERR_SEC_USER_CANCELED).code(),
                "credential.cancelled"
            );
            assert_eq!(
                status_error(ERR_SEC_AUTH_FAILED).code(),
                "credential.denied"
            );
            assert_eq!(
                status_error(ERR_SEC_INTERACTION_REQUIRED).code(),
                "credential.unavailable"
            );
            assert_eq!(
                status_error(ERR_SEC_INTERACTION_NOT_ALLOWED).code(),
                "credential.unavailable"
            );
        }

        #[test]
        fn legacy_query_keeps_the_data_protection_keychain() {
            let reference = SecretRef::new("opaque-test-account").unwrap();
            let query = base_query("com.autologin.credentials", &reference);
            let protection_key = unsafe { sec_string(kSecUseDataProtectionKeychain) };

            assert!(query.contains_key(protection_key.as_CFTypeRef()));
            let (keys, values) = query.get_keys_and_values();
            assert!(keys.iter().zip(values).any(|(key, value)| {
                *key == protection_key.as_CFTypeRef()
                    && value == CFBoolean::true_value().as_CFTypeRef()
            }));
        }

        #[test]
        fn new_query_explicitly_uses_login_keychain() {
            let reference = SecretRef::new("login-keychain:v1:test").unwrap();
            let query = base_query("test", &reference);
            let key = unsafe { sec_string(kSecUseDataProtectionKeychain) };
            let (keys, values) = query.get_keys_and_values();
            assert!(keys
                .iter()
                .zip(values)
                .any(|(k, v)| *k == key.as_CFTypeRef()
                    && v == CFBoolean::false_value().as_CFTypeRef()));
        }

        #[test]
        fn user_presence_flags_match_the_core_foundation_abi_width() {
            fn requires_cf_option_flags(_: CFOptionFlags) {}

            requires_cf_option_flags(K_SEC_ACCESS_CONTROL_USER_PRESENCE);
            assert_eq!(
                std::mem::size_of_val(&K_SEC_ACCESS_CONTROL_USER_PRESENCE),
                std::mem::size_of::<usize>()
            );
        }

        #[tokio::test(flavor = "current_thread")]
        async fn security_dispatcher_runs_the_sync_operation_off_the_async_executor() {
            let caller = std::thread::current().id();
            let worker = run_security_operation(|| Ok(std::thread::current().id()))
                .await
                .unwrap();

            assert_ne!(worker, caller);
        }
    }
}

#[cfg(not(target_os = "macos"))]
#[async_trait]
impl KeychainBackend for MacKeychainBackend {
    async fn put(
        &self,
        _reference: &SecretRef,
        _label: &str,
        _secret: SecretString,
    ) -> Result<(), AppError> {
        Err(AppError::new("platform.unsupported"))
    }

    async fn read(&self, _reference: &SecretRef, _reason: &str) -> Result<SecretString, AppError> {
        Err(AppError::new("platform.unsupported"))
    }

    async fn delete(&self, _reference: &SecretRef) -> Result<(), AppError> {
        Err(AppError::new("platform.unsupported"))
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use super::{KeychainBackend, MacCredentialStore};
    use async_trait::async_trait;
    use autologin_core::{AppError, CredentialStore, SecretRef};
    use secrecy::SecretString;

    fn secret(value: &str) -> SecretString {
        SecretString::from(value.to_owned())
    }

    #[derive(Default)]
    struct FakeKeychain {
        values: Mutex<HashMap<String, SecretString>>,
        read_error: Option<AppError>,
    }

    impl FakeKeychain {
        fn denied() -> Self {
            Self {
                values: Mutex::default(),
                read_error: Some(AppError::credential_denied()),
            }
        }
    }

    #[async_trait]
    impl KeychainBackend for FakeKeychain {
        async fn put(
            &self,
            reference: &SecretRef,
            _label: &str,
            secret: SecretString,
        ) -> Result<(), AppError> {
            self.values
                .lock()
                .expect("fake keychain lock")
                .insert(reference.as_str().to_owned(), secret);
            Ok(())
        }

        async fn read(
            &self,
            reference: &SecretRef,
            _reason: &str,
        ) -> Result<SecretString, AppError> {
            if let Some(error) = &self.read_error {
                return Err(error.clone());
            }
            self.values
                .lock()
                .expect("fake keychain lock")
                .get(reference.as_str())
                .cloned()
                .ok_or_else(|| AppError::new("credential.not_found"))
        }

        async fn delete(&self, reference: &SecretRef) -> Result<(), AppError> {
            self.values
                .lock()
                .expect("fake keychain lock")
                .remove(reference.as_str());
            Ok(())
        }
    }

    #[test]
    fn production_references_select_new_storage_without_colliding_with_legacy_ids() {
        use autologin_core::CredentialStore;
        let store = MacCredentialStore::new("test".into());
        let first = store.new_reference().unwrap();
        let second = store.new_reference().unwrap();
        assert!(first.as_str().starts_with("login-keychain:v1:"));
        assert_ne!(first.as_str(), second.as_str());
    }

    #[tokio::test]
    async fn reveal_propagates_keychain_access_denial() {
        let store = MacCredentialStore::with_backend(FakeKeychain::denied());
        let reference = SecretRef::new("test-reference").unwrap();
        store
            .put(&reference, "example.com", secret("hunter2"))
            .await
            .unwrap();

        let error = store
            .reveal(&reference, "Reveal website password")
            .await
            .unwrap_err();

        assert_eq!(error.code(), "credential.denied");
    }

    #[tokio::test]
    async fn deleting_reference_removes_secret() {
        let store = MacCredentialStore::with_backend(FakeKeychain::default());
        let reference = SecretRef::new("test-reference").unwrap();
        store.put(&reference, "Steam", secret("pw")).await.unwrap();

        store.delete(&reference).await.unwrap();

        assert_eq!(
            store.reveal(&reference, "test").await.unwrap_err().code(),
            "credential.not_found"
        );
    }
}

use async_trait::async_trait;
use autologin_core::{AppError, CredentialStore, SecretRef};
use secrecy::SecretString;

/// Stores LoginDeck secrets as Generic Credentials in the current user's Credential Manager.
pub struct WindowsCredentialStore {
    service: String,
}

impl WindowsCredentialStore {
    pub fn new(service: String) -> Self {
        Self { service }
    }
}

#[cfg(target_os = "windows")]
mod native {
    use std::{ptr, slice};

    use autologin_core::AppError;
    use secrecy::{ExposeSecret, SecretString};
    use windows::{
        core::{Error, PCWSTR, PWSTR},
        Win32::{
            Foundation::ERROR_NOT_FOUND,
            Security::Credentials::{
                CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW,
                CRED_MAX_CREDENTIAL_BLOB_SIZE, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
            },
        },
    };
    use zeroize::Zeroizing;

    use crate::credential_error;

    struct CredentialBuffer(*mut CREDENTIALW);

    impl CredentialBuffer {
        fn credential(&self) -> &CREDENTIALW {
            // SAFETY: CredReadW returned this non-null pointer and the guard owns it until drop.
            unsafe { &*self.0 }
        }
    }

    impl Drop for CredentialBuffer {
        fn drop(&mut self) {
            // SAFETY: The pointer came from a successful CredReadW and is freed exactly once.
            unsafe { CredFree(self.0.cast()) };
        }
    }

    fn wide(value: &str, field: &'static str) -> Result<Vec<u16>, AppError> {
        if value.contains('\0') {
            return Err(AppError::invalid_field(field));
        }
        Ok(value.encode_utf16().chain(std::iter::once(0)).collect())
    }

    fn native_status(error: Error) -> u32 {
        let hresult = error.code().0 as u32;
        if hresult & 0xffff_0000 == 0x8007_0000 {
            hresult & 0x0000_ffff
        } else {
            hresult
        }
    }

    fn read_native(target: &[u16]) -> Result<CredentialBuffer, u32> {
        let mut credential = ptr::null_mut();
        // SAFETY: target is NUL-terminated and output is initialized by CredReadW on success.
        unsafe {
            CredReadW(
                PCWSTR(target.as_ptr()),
                CRED_TYPE_GENERIC,
                None,
                &mut credential,
            )
        }
        .map_err(native_status)?;
        if credential.is_null() {
            return Err(0);
        }
        Ok(CredentialBuffer(credential))
    }

    fn read(target: &[u16]) -> Result<CredentialBuffer, AppError> {
        read_native(target).map_err(|status| {
            if status == 0 {
                AppError::new("credential.unavailable")
            } else {
                credential_error(status)
            }
        })
    }

    pub(super) fn put(target: &str, secret: &SecretString) -> Result<(), AppError> {
        let mut target = wide(target, "secret_ref")?;
        if secret.expose_secret().len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
            return Err(AppError::invalid_field("password"));
        }

        match read_native(&target) {
            Ok(_existing) => return Err(AppError::new("storage.conflict")),
            Err(status) if status == ERROR_NOT_FOUND.0 => {}
            Err(0) => return Err(AppError::new("credential.unavailable")),
            Err(status) => return Err(credential_error(status)),
        }

        let mut plaintext = Zeroizing::new(secret.expose_secret().as_bytes().to_vec());
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(target.as_mut_ptr()),
            CredentialBlobSize: plaintext.len() as u32,
            CredentialBlob: plaintext.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..Default::default()
        };

        // SAFETY: all pointers reference live mutable buffers for the duration of the call.
        unsafe { CredWriteW(&credential, 0) }
            .map_err(native_status)
            .map_err(credential_error)
    }

    pub(super) fn reveal(target: &str) -> Result<SecretString, AppError> {
        let target = wide(target, "secret_ref")?;
        let buffer = read(&target)?;
        let credential = buffer.credential();
        let length = credential.CredentialBlobSize as usize;
        if length > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize
            || (length > 0 && credential.CredentialBlob.is_null())
        {
            return Err(AppError::new("credential.unavailable"));
        }
        let bytes = if length == 0 {
            &[][..]
        } else {
            // SAFETY: CredentialBlob has CredentialBlobSize readable bytes until CredFree.
            unsafe { slice::from_raw_parts(credential.CredentialBlob, length) }
        };
        let plaintext = Zeroizing::new(bytes.to_vec());
        let value =
            std::str::from_utf8(&plaintext).map_err(|_| AppError::new("credential.unavailable"))?;
        Ok(SecretString::from(value.to_owned()))
    }

    pub(super) fn delete(target: &str) -> Result<(), AppError> {
        let target = wide(target, "secret_ref")?;
        // SAFETY: target is a live NUL-terminated UTF-16 buffer for the duration of the call.
        unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None) }
            .map_err(native_status)
            .map_err(credential_error)
    }
}

impl WindowsCredentialStore {
    fn target(&self, reference: &SecretRef) -> String {
        format!("LoginDeck/{}/{}", self.service, reference.as_str())
    }
}

#[async_trait]
impl CredentialStore for WindowsCredentialStore {
    fn new_reference(&self) -> Result<SecretRef, AppError> {
        SecretRef::new(format!("credential-manager:v1:{}", uuid::Uuid::new_v4()))
    }

    async fn put(
        &self,
        reference: &SecretRef,
        _label: &str,
        secret: SecretString,
    ) -> Result<(), AppError> {
        #[cfg(target_os = "windows")]
        {
            native::put(&self.target(reference), &secret)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (reference, secret);
            Err(AppError::new("credential.unavailable"))
        }
    }

    async fn reveal(&self, reference: &SecretRef, _reason: &str) -> Result<SecretString, AppError> {
        #[cfg(target_os = "windows")]
        {
            native::reveal(&self.target(reference))
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = reference;
            Err(AppError::new("credential.unavailable"))
        }
    }

    async fn delete(&self, reference: &SecretRef) -> Result<(), AppError> {
        #[cfg(target_os = "windows")]
        {
            native::delete(&self.target(reference))
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = reference;
            Err(AppError::new("credential.unavailable"))
        }
    }
}

//! Serializes Keychain + SQLite work across desktop and Native Messaging processes.
use crate::AppError;
use std::{fs::File, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{Mutex, OwnedMutexGuard};

pub(crate) struct MutationLock {
    local: Arc<Mutex<()>>,
    path: Option<PathBuf>,
}
pub(crate) struct MutationGuard {
    _local: OwnedMutexGuard<()>,
    _file: Option<File>,
}
impl MutationLock {
    pub(crate) fn new(path: Option<PathBuf>) -> Self {
        Self {
            local: Arc::new(Mutex::new(())),
            path,
        }
    }
    pub(crate) async fn lock(&self) -> Result<MutationGuard, AppError> {
        let local = self.local.clone().lock_owned().await;
        let file = if let Some(path) = &self.path {
            let path = path.clone();
            let file = tokio::task::spawn_blocking(move || {
                let mut options = std::fs::OpenOptions::new();
                options.read(true).write(true).create(true).truncate(false);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                options.open(path)
            })
            .await
            .map_err(|_| AppError::new("storage.database"))?
            .map_err(|_| AppError::new("storage.database"))?;
            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            loop {
                match file.try_lock() {
                    Ok(()) => break,
                    Err(std::fs::TryLockError::WouldBlock)
                        if tokio::time::Instant::now() < deadline =>
                    {
                        tokio::time::sleep(Duration::from_millis(25)).await
                    }
                    Err(std::fs::TryLockError::WouldBlock) => {
                        return Err(AppError::new("storage.conflict"))
                    }
                    Err(_) => return Err(AppError::new("storage.database")),
                }
            }
            Some(file)
        } else {
            None
        };
        Ok(MutationGuard {
            _local: local,
            _file: file,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn waiting_lock_is_cancellable_and_child_process_cannot_take_owned_lock() {
        let path = std::env::temp_dir().join(format!("logindeck-lock-{}", uuid::Uuid::new_v4()));
        let a = MutationLock::new(Some(path.clone()));
        let b = MutationLock::new(Some(path.clone()));
        let held = a.lock().await.unwrap();
        assert!(tokio::time::timeout(Duration::from_millis(70), b.lock())
            .await
            .is_err());
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "mutation_lock::tests::child_lock_probe",
                "--ignored",
            ])
            .env("LOGINDECK_LOCK_PROBE", &path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child process unexpectedly acquired the lock"
        );
        drop(held);
        assert!(b.lock().await.is_ok());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    #[ignore = "invoked by parent lock test with a temporary lock path"]
    fn child_lock_probe() {
        let path = std::env::var_os("LOGINDECK_LOCK_PROBE").expect("parent probe path");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        assert!(matches!(
            file.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
    }
}

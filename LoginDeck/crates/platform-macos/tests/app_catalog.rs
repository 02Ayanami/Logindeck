use std::{fs, path::Path};

use autologin_core::ApplicationCatalog;
use platform_macos::MacApplicationCatalog;
use uuid::Uuid;

#[tokio::test]
async fn import_rejects_a_real_bare_executable_file() {
    let path = std::env::temp_dir().join(format!("autologin-bare-tool-{}", Uuid::new_v4()));
    fs::write(&path, b"not an application bundle").unwrap();
    let catalog = MacApplicationCatalog::new();
    let error = catalog.import_bundle(Path::new(&path)).await.unwrap_err();
    assert_eq!(error.code(), "application.unsupported_import");
    fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn import_rejects_a_symlinked_root_selection() {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::symlink;
        let target = std::env::temp_dir().join(format!("autologin-target-{}", Uuid::new_v4()));
        let link = std::env::temp_dir().join(format!("autologin-link-{}.app", Uuid::new_v4()));
        fs::create_dir_all(&target).unwrap();
        symlink(&target, &link).unwrap();
        let error = MacApplicationCatalog::new()
            .import_bundle(&link)
            .await
            .unwrap_err();
        assert_eq!(error.code(), "application.unsupported_import");
        fs::remove_file(link).unwrap();
        fs::remove_dir(target).unwrap();
    }
}

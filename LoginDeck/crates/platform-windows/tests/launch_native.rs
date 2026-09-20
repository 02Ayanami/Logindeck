//! Controlled child executable: copied filename selects child mode; no launch arguments exist.
#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn main() {
    native::run();
}

#[cfg(windows)]
mod native {
    use autologin_core::{ApplicationCatalog, ApplicationRecord, Platform};
    use platform_windows::WindowsApplicationCatalog;
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{Duration, Instant},
    };

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            // A just-exited image can remain mapped briefly. Only the owned UUID tree is removed.
            for _ in 0..100 {
                if fs::remove_dir_all(&self.0).is_ok() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            if !std::thread::panicking() {
                panic!("controlled launch fixture cleanup failed");
            }
        }
    }

    async fn record(path: &Path) -> ApplicationRecord {
        let found = WindowsApplicationCatalog::new()
            .import_bundle(path)
            .await
            .unwrap()
            .remove(0);
        let mut app = ApplicationRecord::new(
            Platform::Windows,
            found.platform_application_id,
            found.display_name,
        )
        .unwrap();
        app.launch_target = found.launch_target;
        app.signature_identity = found.signature_identity;
        app.discovery_source = found.discovery_source;
        app
    }

    pub(super) fn run() {
        let current = std::env::current_exe().unwrap();
        if current
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("LoginDeck Launch Child")
        {
            let args: Vec<_> = std::env::args().collect();
            assert_eq!(args.len(), 1, "filename must not inject arguments");
            fs::write(
                current.with_extension("proof"),
                std::process::id().to_string(),
            )
            .unwrap();
            return;
        }
        tokio::runtime::Runtime::new().unwrap().block_on(async {
        let directory =
            std::env::temp_dir().join(format!("logindeck-launch-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let fixture = Fixture(directory);
        let path = fixture
            .0
            .join("LoginDeck Launch Child --ignored & calc.exe");
        fs::copy(&current, &path).unwrap();
        let app = record(&path).await;
        let catalog = WindowsApplicationCatalog::new();
        let launched = catalog
            .verify_and_launch(&app)
            .await
            .expect("unchanged controlled child must launch");
        assert_ne!(launched.launched_process_id, 0);
        assert_eq!(launched.verified_launch_target, app.launch_target);
        let deadline = Instant::now() + Duration::from_secs(10);
        let proof = loop {
            if let Ok(proof) = fs::read_to_string(path.with_extension("proof")) {
                break proof;
            }
            assert!(Instant::now() < deadline, "child proof timed out");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(proof.parse::<u32>().unwrap(), launched.launched_process_id);
        // Let the controlled process release its mapped image before mutation assertions.
        for _ in 0..100 {
            if fs::OpenOptions::new().append(true).open(&path).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        fs::write(&path, b"changed identity; invalid image").unwrap();
        assert_eq!(
            catalog.verify_and_launch(&app).await.unwrap_err().code(),
            "application.signature_changed"
        );
        fs::remove_file(&path).unwrap();
        assert_eq!(
            catalog.verify_and_launch(&app).await.unwrap_err().code(),
            "application.not_found"
        );
        for target in [
            "cmd.exe /c calc",
            "exe:C:\\fixture.exe --payload",
            "exe:C:\\fixture.exe\" --payload.exe",
            "exe:\\\\server\\fixture.exe",
        ] {
            let mut invalid = app.clone();
            invalid.launch_target = target.into();
            let error = catalog.verify_and_launch(&invalid).await.unwrap_err();
            assert_eq!(error.code(), "application.unsupported_target");
            assert!(error.params.is_empty());
        }
        let inert = fixture.0.join("Inert.exe");
        fs::write(&inert, b"not an executable image").unwrap();
        let inert_record = record(&inert).await;
        assert_eq!(
            catalog
                .verify_and_launch(&inert_record)
                .await
                .unwrap_err()
                .code(),
            "application.launch_failed"
        );
        // Replace a previously safe ancestor with a real junction. Its target is
        // another directory inside this owned test tree and must remain untouched.
        let original = fixture.0.join("original");
        let displaced = fixture.0.join("displaced");
        fs::create_dir(&original).unwrap();
        let nested = original.join("Nested.exe");
        fs::write(&nested, b"inert before junction").unwrap();
        let nested_record = record(&nested).await;
        fs::rename(&original, &displaced).unwrap();
        let result = std::process::Command::new("cmd").args(["/D", "/C", "mklink", "/J"])
            .arg(&original).arg(&displaced).output().unwrap();
        assert!(result.status.success(), "controlled junction creation failed");
        assert_eq!(catalog.verify_and_launch(&nested_record).await.unwrap_err().code(), "application.not_found");
        fs::remove_dir(&original).unwrap();
        assert_eq!(fs::read(displaced.join("Nested.exe")).unwrap(), b"inert before junction");
        println!(
            "launch_native: controlled PID/argv, changed/missing/raw input, junction and native failure passed"
        );
    });
    }
}

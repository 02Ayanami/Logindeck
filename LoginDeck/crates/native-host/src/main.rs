use autologin_core::{SqliteRepositories, VaultService};
use autologin_native_host::{decode, read_frame, trusted_extension, write_frame, Session};
use std::{
    io,
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};

use platform_runtime::{application_data_directory, NativeCredentialStore};
fn main() {
    // Native host may only be launched for this exact unpacked/store identity.
    if !std::env::args()
        .nth(1)
        .is_some_and(|x| trusted_extension(&x))
    {
        std::process::exit(1);
    }
    if run().is_err() {
        eprintln!("LoginDeck native host stopped");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    // No caller-controlled database path, service, shell command, or credential-read operation.
    let directory = application_data_directory()?;
    std::fs::create_dir_all(&directory)?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let repos = rt.block_on(SqliteRepositories::connect(&format!(
        "sqlite://{}",
        directory.join("autologin.sqlite3").display()
    )))?;
    rt.block_on(repos.migrate())?;
    let vault = VaultService::new(
        repos.websites(),
        Arc::new(NativeCredentialStore::new("com.autologin.app".into())),
    );
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut input = io::stdin().lock();
        while let Ok(Some(frame)) = read_frame(&mut input) {
            if tx.send(frame).is_err() {
                break;
            }
        }
    });
    let mut session = Session::default();
    let mut output = io::stdout().lock();
    let instance_path = directory.join("desktop-instance.lock");
    let mut connected = false;
    let mut heartbeat = Instant::now();
    loop {
        session.expire(std::time::Instant::now());
        if !rt.block_on(repos.settings().browser_capture())?.enabled {
            session.clear();
        }
        // A live native messaging port remains connected even on idle/internal tabs.
        // Stop renewing the lease when stdin closes or the desktop stops running.
        if connected && heartbeat.elapsed() >= Duration::from_secs(2) {
            if std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&instance_path)
                .ok()
                .is_some_and(|file| file.try_lock().is_err())
            {
                rt.block_on(repos.settings().note_browser_connection())?;
            }
            heartbeat = Instant::now();
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(bytes) => {
                let request = decode(&bytes).map_err(|_| "invalid request")?;
                drop(bytes);
                let desktop_running = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&instance_path)
                    .ok()
                    .is_some_and(|file| file.try_lock().is_err());
                if !desktop_running {
                    write_frame(
                        &mut output,
                        &serde_json::json!({"version":1,"requestId":request.request_id,"type":"status","enabled":false,"connected":false,"uiLocale":"en","revision":0}),
                    )?;
                    continue;
                }
                if matches!(&request.body, autologin_native_host::Body::Status {}) {
                    rt.block_on(repos.settings().note_browser_connection())?;
                    connected = true;
                    heartbeat = Instant::now();
                }
                let locale = rt.block_on(repos.settings().locale())?;
                let locale = match locale {
                    autologin_core::Locale::ZhCn => "zh-CN",
                    _ => "en",
                };
                let response = rt.block_on(session.handle(request, &vault, locale));
                write_frame(&mut output, &response)?;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    session.clear();
    Ok(())
}

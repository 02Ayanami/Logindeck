//! Discovery, import and atomic verified launching of macOS application bundles.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use async_trait::async_trait;
use autologin_core::{
    AppError, ApplicationCatalog, ApplicationRecord, DiscoveredApplication, DiscoverySource,
    Platform, VerifiedApplication,
};

use crate::bookmarks::{BookmarkCreator, MacBookmarks};

const UNSUPPORTED_IMPORT: &str = "application.unsupported_import";

#[derive(Clone, Debug, Eq, PartialEq)]
struct BundleCandidate {
    path: PathBuf,
    display_name: String,
    bundle_id: String,
    version: Option<String>,
    signature_identity: String,
    package_type: String,
    activation_policy: ActivationPolicy,
    has_executable: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
enum ActivationPolicy {
    Regular,
    Accessory,
    Prohibited,
}

impl BundleCandidate {
    fn user_facing(&self) -> bool {
        self.package_type == "APPL"
            && self.activation_policy == ActivationPolicy::Regular
            && self.has_executable
            && self
                .path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
            && !self.bundle_id.trim().is_empty()
            && !looks_like_support_bundle(&self.display_name, &self.path)
    }
}

fn looks_like_support_bundle(display_name: &str, path: &Path) -> bool {
    let filename = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    [
        "helper",
        "updater",
        "uninstaller",
        "uninstall",
        "background",
        "agent",
    ]
    .iter()
    .any(|suffix| is_support_name(filename, suffix) || is_support_name(display_name, suffix))
}

fn is_support_name(name: &str, suffix: &str) -> bool {
    let name = name.trim();
    let lower = name.to_ascii_lowercase();
    if lower == suffix {
        return true;
    }
    if [" ", "-", "_"]
        .iter()
        .any(|separator| lower.ends_with(&format!("{separator}{suffix}")))
    {
        return true;
    }
    // A CamelCase suffix such as FooHelper is a support bundle; an unrelated interior token
    // (e.g. LegitimateAgentName) is not.
    let start = name.len().saturating_sub(suffix.len());
    name.get(start..).is_some_and(|tail| {
        tail.eq_ignore_ascii_case(suffix)
            && tail.chars().next().is_some_and(char::is_uppercase)
            && name[..start].chars().last().is_some_and(char::is_lowercase)
    })
}

#[async_trait]
trait AppSource: Send + Sync {
    async fn registered_apps(&self) -> Result<Vec<BundleCandidate>, AppError>;
    async fn scan_standard_directories(&self) -> Result<Vec<BundleCandidate>, AppError>;
    async fn import_selection(&self, path: &Path) -> Result<Vec<BundleCandidate>, AppError>;
}

#[derive(Clone)]
struct CancelOnDrop {
    flag: Arc<AtomicBool>,
    armed: bool,
}
impl CancelOnDrop {
    fn new(flag: Arc<AtomicBool>) -> Self {
        Self { flag, armed: true }
    }
    fn disarm(mut self) {
        self.armed = false;
    }
}
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.flag.store(true, Ordering::Release);
        }
    }
}

#[derive(Clone)]
struct LaunchAttestation {
    target: String,
    process_id: u32,
}
#[async_trait]
trait AtomicVerifier: Send + Sync {
    /// Resolves, prechecks, launches, then attests the actual PID before credential use.
    async fn verify_and_launch(
        &self,
        app: &ApplicationRecord,
        cancelled: Arc<AtomicBool>,
    ) -> Result<LaunchAttestation, AppError>;
}

/// macOS application catalogue. Its only public operation that can launch an application is the
/// identity-checking `ApplicationCatalog::verify_and_launch` operation.
pub struct MacApplicationCatalog {
    source: Arc<dyn AppSource>,
    bookmarks: Arc<dyn BookmarkCreator>,
    verifier: Arc<dyn AtomicVerifier>,
}

impl MacApplicationCatalog {
    pub fn new() -> Self {
        Self {
            source: Arc::new(NativeAppSource),
            bookmarks: Arc::new(MacBookmarks::new()),
            verifier: Arc::new(NativeAtomicVerifier),
        }
    }

    #[cfg(test)]
    fn from_parts(
        source: Arc<dyn AppSource>,
        bookmarks: Arc<dyn BookmarkCreator>,
        verifier: Arc<dyn AtomicVerifier>,
    ) -> Self {
        Self {
            source,
            bookmarks,
            verifier,
        }
    }

    fn discovery_record(
        &self,
        bundle: BundleCandidate,
        alternates: Vec<String>,
        source: DiscoverySource,
    ) -> Result<DiscoveredApplication, AppError> {
        let path_access_ref =
            if source == DiscoverySource::ManualImport && !is_standard_location(&bundle.path) {
                Some(self.bookmarks.create(&bundle.path)?)
            } else {
                None
            };
        Ok(DiscoveredApplication {
            platform: Platform::Macos,
            display_name: bundle.display_name,
            platform_application_id: bundle.bundle_id,
            launch_target: bundle.path.to_string_lossy().into_owned(),
            alternate_launch_targets: alternates,
            path_access_ref,
            version: bundle.version,
            signature_identity: bundle.signature_identity,
            discovery_source: source,
        })
    }

    async fn records_from(
        &self,
        candidates: Vec<BundleCandidate>,
        source: DiscoverySource,
    ) -> Result<Vec<DiscoveredApplication>, AppError> {
        let deduplicated = tokio::task::spawn_blocking(move || deduplicate(candidates))
            .await
            .map_err(|_| AppError::new("application.discovery_unavailable"))?;
        deduplicated
            .into_iter()
            .map(|(bundle, alternates)| self.discovery_record(bundle, alternates, source))
            .collect()
    }
}

impl Default for MacApplicationCatalog {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ApplicationCatalog for MacApplicationCatalog {
    async fn discover(&self) -> Result<Vec<DiscoveredApplication>, AppError> {
        // Indexed candidates are best-effort; standard locations remain available if the index fails.
        let registered = match self.source.registered_apps().await {
            Err(error) if error.code() == "application.scan_timeout" => return Err(error),
            result => result.unwrap_or_default(),
        };
        let scanned = self.source.scan_standard_directories().await?;
        self.records_from(
            registered
                .into_iter()
                .chain(scanned)
                .filter(|bundle| bundle.user_facing() && is_automatic_location(&bundle.path))
                .collect(),
            DiscoverySource::Automatic,
        )
        .await
    }

    async fn import_bundle(&self, path: &Path) -> Result<Vec<DiscoveredApplication>, AppError> {
        // Selecting a bare executable is never an import path. A directory selection is the only
        // input that may trigger recursive discovery, and native scanning does not follow links.
        let candidates = self.source.import_selection(path).await?;
        let candidates: Vec<_> = candidates
            .into_iter()
            .filter(BundleCandidate::user_facing)
            .collect();
        if candidates.is_empty() {
            return Err(AppError::new(UNSUPPORTED_IMPORT));
        }
        self.records_from(candidates, DiscoverySource::ManualImport)
            .await
    }

    async fn verify_and_launch(
        &self,
        app: &ApplicationRecord,
    ) -> Result<VerifiedApplication, AppError> {
        if app.platform != Platform::Macos {
            return Err(AppError::application_not_found());
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let guard = CancelOnDrop::new(cancelled.clone());
        let attestation = self.verifier.verify_and_launch(app, cancelled).await?;
        guard.disarm();
        Ok(VerifiedApplication {
            application: app.clone(),
            verified_launch_target: attestation.target,
            launched_process_id: attestation.process_id,
        })
    }
}

fn deduplicate(candidates: Vec<BundleCandidate>) -> Vec<(BundleCandidate, Vec<String>)> {
    let mut grouped = BTreeMap::<String, BTreeMap<String, BundleCandidate>>::new();
    for mut candidate in candidates {
        // Canonical paths collapse symlink aliases before choosing a deterministic target. If the
        // path disappeared after discovery, retain the original spelling for display only.
        candidate.path = candidate.path.canonicalize().unwrap_or(candidate.path);
        let key = candidate.path.to_string_lossy().into_owned();
        grouped
            .entry(candidate.bundle_id.clone())
            .or_default()
            .entry(key)
            .or_insert(candidate);
    }
    grouped
        .into_values()
        .map(|paths| {
            let mut group: Vec<_> = paths.into_values().collect();
            group.sort_by_key(|candidate| canonical_key(&candidate.path));
            let canonical = group.remove(0);
            let alternates = group
                .into_iter()
                .map(|candidate| candidate.path.to_string_lossy().into_owned())
                .collect();
            (canonical, alternates)
        })
        .collect()
}

fn canonical_key(path: &Path) -> (u8, String) {
    let value = path.to_string_lossy().into_owned();
    let rank = if value.starts_with("/Applications/") {
        0
    } else if value.starts_with("/System/Applications/") {
        1
    } else if home_applications().is_some_and(|home| path.starts_with(home)) {
        2
    } else {
        3
    };
    (rank, value)
}

fn home_applications() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Applications"))
}

fn is_automatic_location(path: &Path) -> bool {
    path.starts_with("/Applications")
        || home_applications().is_some_and(|home| path.starts_with(home))
}

fn is_standard_location(path: &Path) -> bool {
    path.starts_with("/Applications")
        || path.starts_with("/System/Applications")
        || home_applications().is_some_and(|home| path.starts_with(home))
}

struct NativeAppSource;
struct NativeAtomicVerifier;

#[cfg(not(target_os = "macos"))]
#[async_trait]
impl AppSource for NativeAppSource {
    async fn registered_apps(&self) -> Result<Vec<BundleCandidate>, AppError> {
        Ok(Vec::new())
    }
    async fn scan_standard_directories(&self) -> Result<Vec<BundleCandidate>, AppError> {
        Ok(Vec::new())
    }
    async fn import_selection(&self, _path: &Path) -> Result<Vec<BundleCandidate>, AppError> {
        Ok(Vec::new())
    }
}

#[cfg(not(target_os = "macos"))]
#[async_trait]
impl AtomicVerifier for NativeAtomicVerifier {
    async fn verify_and_launch(
        &self,
        _app: &ApplicationRecord,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<LaunchAttestation, AppError> {
        Err(AppError::application_not_found())
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::{
        collections::HashSet,
        ffi::{c_char, c_void, CStr, CString},
        fs,
        io::{self, Read},
        os::unix::ffi::OsStrExt,
        os::unix::io::AsRawFd,
        os::unix::process::CommandExt,
        path::Path,
        process::{Child, Command, Stdio},
        ptr,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            mpsc, Arc,
        },
        thread,
        time::{Duration, Instant},
    };

    use async_trait::async_trait;
    use autologin_core::{AppError, ApplicationRecord};
    use tokio::task;

    use crate::bookmarks::stable_application_url;

    use super::{
        ActivationPolicy, AppSource, AtomicVerifier, BundleCandidate, LaunchAttestation,
        NativeAppSource, NativeAtomicVerifier,
    };

    type CFTypeRef = *const c_void;
    type CFURLRef = *const c_void;
    type CFBundleRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFIndex = isize;
    type OSStatus = i32;
    const UTF8: u32 = 0x0800_0100;
    const K_SEC_CS_STRICT_VALIDATE: u32 = 1 << 4;
    const INDEXED_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);
    const INDEXED_DISCOVERY_STDOUT_LIMIT: usize = 1024 * 1024;
    const INDEXED_DISCOVERY_STDERR_LIMIT: usize = 1024 * 1024;
    const INDEXED_DISCOVERY_TOTAL_LIMIT: usize = 2 * 1024 * 1024;
    const PROCESS_READ_CHUNK: usize = 8192;
    const SIGKILL: i32 = 9;
    const F_GETFL: i32 = 3;
    const F_SETFL: i32 = 4;
    const O_NONBLOCK: i32 = 0x0004;

    unsafe extern "C" {
        fn setpgid(pid: i32, pgid: i32) -> i32;
        fn kill(pid: i32, signal: i32) -> i32;
        fn fcntl(fd: i32, command: i32, argument: i32) -> i32;
    }

    #[derive(Clone, Copy)]
    struct ProcessLimits {
        stdout: usize,
        stderr: usize,
        total: usize,
    }

    impl ProcessLimits {
        const fn new(stdout: usize, stderr: usize, total: usize) -> Self {
            Self {
                stdout,
                stderr,
                total,
            }
        }

        const fn indexed_discovery() -> Self {
            Self::new(
                INDEXED_DISCOVERY_STDOUT_LIMIT,
                INDEXED_DISCOVERY_STDERR_LIMIT,
                INDEXED_DISCOVERY_TOTAL_LIMIT,
            )
        }
    }

    enum BoundedProcessError {
        Spawn,
        Read,
        Wait,
        Timeout,
        OutputLimit,
        Nonzero,
    }

    enum ReaderEvent {
        Complete,
        Failed,
        LimitExceeded,
        Timeout,
    }

    struct DrainedOutput {
        bytes: Vec<u8>,
        exceeded_limit: bool,
        deadline_reached: bool,
    }

    struct DrainControl {
        stop: AtomicBool,
        deadline: Instant,
    }

    fn set_nonblocking(fd: i32) -> io::Result<()> {
        unsafe {
            let flags = fcntl(fd, F_GETFL, 0);
            if flags < 0 || fcntl(fd, F_SETFL, flags | O_NONBLOCK) < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }

    fn reserve_total(total: &AtomicUsize, limit: usize, requested: usize) -> usize {
        let mut current = total.load(Ordering::Acquire);
        loop {
            let reserved = requested.min(limit.saturating_sub(current));
            match total.compare_exchange_weak(
                current,
                current + reserved,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return reserved,
                Err(observed) => current = observed,
            }
        }
    }

    fn drain_pipe<R: Read + AsRawFd>(
        mut pipe: R,
        stream_limit: usize,
        total_limit: usize,
        total: Arc<AtomicUsize>,
        events: mpsc::Sender<ReaderEvent>,
        control: Arc<DrainControl>,
    ) -> io::Result<DrainedOutput> {
        set_nonblocking(pipe.as_raw_fd())?;
        let mut bytes = Vec::new();
        let mut exceeded_limit = false;
        let mut buffer = [0_u8; PROCESS_READ_CHUNK];

        loop {
            if control.stop.load(Ordering::Acquire) {
                return Ok(DrainedOutput {
                    bytes,
                    exceeded_limit,
                    deadline_reached: false,
                });
            }
            if Instant::now() >= control.deadline {
                control.stop.store(true, Ordering::Release);
                let _ = events.send(ReaderEvent::Timeout);
                return Ok(DrainedOutput {
                    bytes,
                    exceeded_limit,
                    deadline_reached: true,
                });
            }
            // Once a limit is crossed, keep draining until EOF so pipe writers can exit. Before
            // that point, read only one byte past the remaining capacity to detect overflow
            // without buffering an unbounded amount.
            let read_limit = if exceeded_limit {
                buffer.len()
            } else {
                stream_limit
                    .saturating_sub(bytes.len())
                    .saturating_add(1)
                    .min(buffer.len())
            };
            let read = match pipe.read(&mut buffer[..read_limit]) {
                Ok(read) => read,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(1));
                    continue;
                }
                Err(error) => return Err(error),
            };
            if read == 0 {
                return Ok(DrainedOutput {
                    bytes,
                    exceeded_limit,
                    deadline_reached: false,
                });
            }
            if exceeded_limit {
                continue;
            }

            let stream_capacity = stream_limit.saturating_sub(bytes.len());
            let permitted = reserve_total(&total, total_limit, read.min(stream_capacity));
            let accepted = permitted.min(stream_capacity);
            bytes.extend_from_slice(&buffer[..accepted]);
            if accepted != read {
                exceeded_limit = true;
                let _ = events.send(ReaderEvent::LimitExceeded);
            }
        }
    }

    fn start_pipe_drain<R: Read + AsRawFd + Send + 'static>(
        pipe: R,
        stream_limit: usize,
        total_limit: usize,
        total: Arc<AtomicUsize>,
        events: mpsc::Sender<ReaderEvent>,
        control: Arc<DrainControl>,
    ) -> io::Result<thread::JoinHandle<io::Result<DrainedOutput>>> {
        thread::Builder::new()
            .name("mdfind-pipe-drain".into())
            .spawn(move || {
                let result = drain_pipe(
                    pipe,
                    stream_limit,
                    total_limit,
                    total,
                    events.clone(),
                    control,
                );
                if result.is_err() {
                    let _ = events.send(ReaderEvent::Failed);
                } else {
                    let _ = events.send(ReaderEvent::Complete);
                }
                result
            })
    }

    fn kill_and_reap(child: &mut Child) -> Result<(), BoundedProcessError> {
        // The child is its own process-group leader, so this also kills shell descendants in the
        // test runner and any unexpected descendants that inherited a pipe descriptor.
        let process_group = -(child.id() as i32);
        unsafe {
            let _ = kill(process_group, SIGKILL);
        }
        let _ = child.kill();
        child
            .wait()
            .map(|_| ())
            .map_err(|_| BoundedProcessError::Wait)
    }

    fn run_bounded_process(
        mut command: Command,
        timeout: Duration,
        limits: ProcessLimits,
    ) -> Result<Vec<u8>, BoundedProcessError> {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        unsafe {
            command.pre_exec(|| {
                if setpgid(0, 0) == 0 {
                    Ok(())
                } else {
                    Err(io::Error::last_os_error())
                }
            });
        }
        let mut child = command.spawn().map_err(|_| BoundedProcessError::Spawn)?;
        let stdout = match child.stdout.take() {
            Some(pipe) => pipe,
            None => {
                let _ = kill_and_reap(&mut child);
                return Err(BoundedProcessError::Read);
            }
        };
        let stderr = match child.stderr.take() {
            Some(pipe) => pipe,
            None => {
                let _ = kill_and_reap(&mut child);
                return Err(BoundedProcessError::Read);
            }
        };

        let (event_sender, event_receiver) = mpsc::channel();
        let total = Arc::new(AtomicUsize::new(0));
        let control = Arc::new(DrainControl {
            stop: AtomicBool::new(false),
            deadline: Instant::now() + timeout,
        });
        let stdout_reader = start_pipe_drain(
            stdout,
            limits.stdout,
            limits.total,
            total.clone(),
            event_sender.clone(),
            control.clone(),
        )
        .map_err(|_| {
            let _ = kill_and_reap(&mut child);
            BoundedProcessError::Read
        })?;
        let stderr_reader = match start_pipe_drain(
            stderr,
            limits.stderr,
            limits.total,
            total,
            event_sender,
            control.clone(),
        ) {
            Ok(reader) => reader,
            Err(_) => {
                control.stop.store(true, Ordering::Release);
                let _ = kill_and_reap(&mut child);
                let _ = stdout_reader.join();
                return Err(BoundedProcessError::Read);
            }
        };

        let started = Instant::now();
        let mut failure = None;
        let mut status = None;
        let mut completed_readers = 0;
        loop {
            match event_receiver.try_recv() {
                Ok(ReaderEvent::Complete) => completed_readers += 1,
                Ok(ReaderEvent::LimitExceeded) => {
                    failure = Some(BoundedProcessError::OutputLimit);
                    break;
                }
                Ok(ReaderEvent::Failed) => {
                    failure = Some(BoundedProcessError::Read);
                    break;
                }
                Ok(ReaderEvent::Timeout) => {
                    failure = Some(BoundedProcessError::Timeout);
                    break;
                }
                Err(mpsc::TryRecvError::Disconnected) | Err(mpsc::TryRecvError::Empty) => {}
            }
            if status.is_none() {
                match child.try_wait() {
                    Ok(Some(child_status)) => status = Some(child_status),
                    Ok(None) => {}
                    Err(_) => {
                        failure = Some(BoundedProcessError::Wait);
                        break;
                    }
                }
            }
            if status.is_some() && completed_readers == 2 {
                break;
            }
            let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
                failure = Some(BoundedProcessError::Timeout);
                break;
            };
            match event_receiver.recv_timeout(remaining.min(Duration::from_millis(10))) {
                Ok(ReaderEvent::Complete) => completed_readers += 1,
                Ok(ReaderEvent::LimitExceeded) => {
                    failure = Some(BoundedProcessError::OutputLimit);
                    break;
                }
                Ok(ReaderEvent::Failed) => {
                    failure = Some(BoundedProcessError::Read);
                    break;
                }
                Ok(ReaderEvent::Timeout) => {
                    failure = Some(BoundedProcessError::Timeout);
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {}
            }
        }

        if failure.is_some() {
            control.stop.store(true, Ordering::Release);
            let _ = kill_and_reap(&mut child);
        }
        let stdout = stdout_reader
            .join()
            .map_err(|_| BoundedProcessError::Read)?
            .map_err(|_| BoundedProcessError::Read)?;
        let stderr = stderr_reader
            .join()
            .map_err(|_| BoundedProcessError::Read)?
            .map_err(|_| BoundedProcessError::Read)?;
        if let Some(error) = failure {
            return Err(error);
        }
        if stdout.deadline_reached || stderr.deadline_reached {
            control.stop.store(true, Ordering::Release);
            let _ = kill_and_reap(&mut child);
            return Err(BoundedProcessError::Timeout);
        }
        if stdout.exceeded_limit || stderr.exceeded_limit {
            return Err(BoundedProcessError::OutputLimit);
        }
        if !status.expect("completed process has a status").success() {
            return Err(BoundedProcessError::Nonzero);
        }
        Ok(stdout.bytes)
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFAllocatorDefault: CFTypeRef;
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: CFTypeRef,
            bytes: *const u8,
            length: CFIndex,
            directory: u8,
        ) -> CFURLRef;
        fn CFBundleCreate(allocator: CFTypeRef, url: CFURLRef) -> CFBundleRef;
        fn CFBundleGetIdentifier(bundle: CFBundleRef) -> CFStringRef;
        fn CFBundleGetValueForInfoDictionaryKey(bundle: CFBundleRef, key: CFStringRef)
            -> CFTypeRef;
        fn CFStringCreateWithCString(
            allocator: CFTypeRef,
            value: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFGetTypeID(value: CFTypeRef) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFBooleanGetTypeID() -> usize;
        fn CFBooleanGetValue(value: CFTypeRef) -> u8;
        fn CFStringGetCString(
            string: CFStringRef,
            buffer: *mut c_char,
            size: CFIndex,
            encoding: u32,
        ) -> u8;
        fn CFRelease(value: CFTypeRef);
        fn CFNumberCreate(
            allocator: CFTypeRef,
            number_type: i32,
            value: *const c_void,
        ) -> CFTypeRef;
        fn CFDictionaryCreateMutable(
            allocator: CFTypeRef,
            capacity: CFIndex,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> CFTypeRef;
        fn CFDictionarySetValue(dictionary: CFTypeRef, key: CFTypeRef, value: CFTypeRef);
        fn CFDictionaryGetValue(dictionary: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    }
    #[link(name = "Security", kind = "framework")]
    extern "C" {
        static kSecGuestAttributePid: CFStringRef;
        static kSecCodeInfoIdentifier: CFStringRef;
        fn SecStaticCodeCreateWithPath(
            path: CFURLRef,
            flags: u32,
            code: *mut CFTypeRef,
        ) -> OSStatus;
        fn SecStaticCodeCheckValidity(
            code: CFTypeRef,
            flags: u32,
            requirement: CFTypeRef,
        ) -> OSStatus;
        fn SecCodeCopyDesignatedRequirement(
            code: CFTypeRef,
            flags: u32,
            requirement: *mut CFTypeRef,
        ) -> OSStatus;
        fn SecRequirementCopyString(
            requirement: CFTypeRef,
            flags: u32,
            text: *mut CFStringRef,
        ) -> OSStatus;
        fn SecCodeCopyGuestWithAttributes(
            host: CFTypeRef,
            attributes: CFTypeRef,
            flags: u32,
            guest: *mut CFTypeRef,
        ) -> OSStatus;
        fn SecCodeCheckValidity(code: CFTypeRef, flags: u32, requirement: CFTypeRef) -> OSStatus;
        fn SecCodeCopySigningInformation(
            code: CFTypeRef,
            flags: u32,
            information: *mut CFTypeRef,
        ) -> OSStatus;
        fn SecRequirementCreateWithString(
            text: CFStringRef,
            flags: u32,
            requirement: *mut CFTypeRef,
        ) -> OSStatus;
    }

    fn not_found() -> AppError {
        AppError::application_not_found()
    }
    unsafe fn file_url(path: &Path) -> Result<CFURLRef, AppError> {
        let bytes = path.as_os_str().as_bytes();
        let url = CFURLCreateFromFileSystemRepresentation(
            kCFAllocatorDefault,
            bytes.as_ptr(),
            bytes.len() as CFIndex,
            1,
        );
        if url.is_null() {
            Err(not_found())
        } else {
            Ok(url)
        }
    }
    unsafe fn string(value: CFStringRef) -> Result<String, AppError> {
        if value.is_null() || CFGetTypeID(value) != CFStringGetTypeID() {
            return Err(not_found());
        }
        let mut buffer = vec![0_i8; 8192];
        if CFStringGetCString(value, buffer.as_mut_ptr(), buffer.len() as CFIndex, UTF8) == 0 {
            return Err(not_found());
        }
        CStr::from_ptr(buffer.as_ptr())
            .to_str()
            .map(str::to_owned)
            .map_err(|_| not_found())
    }
    unsafe fn info_string(bundle: CFBundleRef, key: &str) -> Option<String> {
        let key = CString::new(key).ok()?;
        let key = CFStringCreateWithCString(kCFAllocatorDefault, key.as_ptr(), UTF8);
        if key.is_null() {
            return None;
        }
        let value = CFBundleGetValueForInfoDictionaryKey(bundle, key);
        CFRelease(key);
        string(value).ok()
    }
    unsafe fn info_bool(bundle: CFBundleRef, key: &str) -> bool {
        let key = match CString::new(key) {
            Ok(value) => value,
            Err(_) => return false,
        };
        let key = CFStringCreateWithCString(kCFAllocatorDefault, key.as_ptr(), UTF8);
        if key.is_null() {
            return false;
        }
        let value = CFBundleGetValueForInfoDictionaryKey(bundle, key);
        CFRelease(key);
        !value.is_null()
            && CFGetTypeID(value) == CFBooleanGetTypeID()
            && CFBooleanGetValue(value) != 0
    }
    fn signature(url: CFURLRef) -> Result<String, AppError> {
        unsafe {
            let mut code = ptr::null();
            if SecStaticCodeCreateWithPath(url, 0, &mut code) != 0 || code.is_null() {
                return Err(AppError::application_signature_changed());
            }
            // Discovery only records the designated identity. Resource hashing of every installed
            // bundle is expensive and belongs to validate_persisted_requirement before launch.
            // No credential use or launch is authorized by this catalog snapshot.
            let mut requirement = ptr::null();
            if SecCodeCopyDesignatedRequirement(code, 0, &mut requirement) != 0
                || requirement.is_null()
            {
                CFRelease(code);
                return Err(AppError::application_signature_changed());
            }
            let mut value = ptr::null();
            let status = SecRequirementCopyString(requirement, 0, &mut value);
            CFRelease(requirement);
            CFRelease(code);
            if status != 0 || value.is_null() {
                return Err(AppError::application_signature_changed());
            }
            let result = string(value);
            CFRelease(value);
            result
        }
    }
    fn inspect_url(
        url: CFURLRef,
        path: &Path,
        capture_signature_identity: bool,
    ) -> Result<BundleCandidate, AppError> {
        if !path.is_dir()
            || !path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("app"))
        {
            return Err(not_found());
        }
        unsafe {
            let bundle = CFBundleCreate(kCFAllocatorDefault, url);
            if bundle.is_null() {
                return Err(not_found());
            }
            let bundle_id = string(CFBundleGetIdentifier(bundle));
            let display_name = info_string(bundle, "CFBundleDisplayName")
                .or_else(|| info_string(bundle, "CFBundleName"))
                .unwrap_or_else(|| {
                    path.file_stem()
                        .and_then(|x| x.to_str())
                        .unwrap_or_default()
                        .to_owned()
                });
            let version = info_string(bundle, "CFBundleShortVersionString");
            let package_type = info_string(bundle, "CFBundlePackageType").unwrap_or_default();
            let activation_policy = if info_bool(bundle, "LSBackgroundOnly") {
                ActivationPolicy::Prohibited
            } else if info_bool(bundle, "LSUIElement") {
                ActivationPolicy::Accessory
            } else {
                ActivationPolicy::Regular
            };
            let executable_name = info_string(bundle, "CFBundleExecutable");
            CFRelease(bundle);
            let bundle_id = bundle_id?;
            let signature_identity = if capture_signature_identity {
                signature(url)?
            } else {
                String::new()
            };
            let has_executable = executable_name
                .is_some_and(|name| path.join("Contents/MacOS").join(name).is_file());
            Ok(BundleCandidate {
                path: path.to_path_buf(),
                display_name,
                bundle_id,
                version,
                signature_identity,
                package_type,
                activation_policy,
                has_executable,
            })
        }
    }
    fn inspect(path: &Path) -> Result<BundleCandidate, AppError> {
        unsafe {
            let url = file_url(path)?;
            let result = inspect_url(url, path, true);
            CFRelease(url);
            result
        }
    }

    fn shallow(
        directory: &Path,
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Vec<BundleCandidate> {
        fs::read_dir(directory)
            .into_iter()
            .flatten()
            .flatten()
            .take_while(|_| !cancelled.load(Ordering::Acquire) && Instant::now() < deadline)
            .filter_map(|entry| {
                let file_type = entry.file_type().ok()?;
                if file_type.is_symlink() || !file_type.is_dir() {
                    return None;
                }
                inspect(&entry.path()).ok()
            })
            .collect()
    }
    fn recursively_selected(directory: &Path) -> Vec<BundleCandidate> {
        let mut result = Vec::new();
        let mut pending = vec![directory.to_path_buf()];
        let mut seen = std::collections::HashSet::new();
        while let Some(current) = pending.pop() {
            let Ok(canonical) = current.canonicalize() else {
                continue;
            };
            if !seen.insert(canonical) {
                continue;
            }
            let Ok(entries) = fs::read_dir(&current) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                if kind.is_symlink() {
                    continue;
                }
                let path = entry.path();
                if kind.is_dir() {
                    if path
                        .extension()
                        .is_some_and(|x| x.eq_ignore_ascii_case("app"))
                    {
                        if let Ok(bundle) = inspect(&path) {
                            result.push(bundle);
                        }
                    } else {
                        pending.push(path);
                    }
                }
            }
        }
        result
    }
    #[async_trait]
    impl AppSource for NativeAppSource {
        async fn registered_apps(&self) -> Result<Vec<BundleCandidate>, AppError> {
            // Spotlight's public metadata index is a supported indexed-app source. The predicate
            // is fixed here; neither user input nor a shell is involved.
            let cancelled = Arc::new(AtomicBool::new(false));
            let _cancel_on_drop = super::CancelOnDrop::new(cancelled.clone());
            task::spawn_blocking(move || {
                let deadline = Instant::now() + Duration::from_secs(25);
                let mut command = Command::new("/usr/bin/mdfind");
                command
                    .arg("-0")
                    .arg("kMDItemContentType == 'com.apple.application-bundle'");
                let stdout = run_bounded_process(
                    command,
                    INDEXED_DISCOVERY_TIMEOUT,
                    ProcessLimits::indexed_discovery(),
                )
                .map_err(|_| AppError::new("application.discovery_unavailable"))?;
                let mut apps = Vec::new();
                for bytes in stdout
                    .split(|byte| *byte == 0)
                    .filter(|bytes| !bytes.is_empty())
                {
                    if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
                        return Err(AppError::new("application.scan_timeout"));
                    }
                    use std::os::unix::ffi::OsStrExt;
                    let path = Path::new(std::ffi::OsStr::from_bytes(bytes));
                    if !super::is_automatic_location(path) {
                        continue;
                    }
                    if let Ok(app) = inspect(path) {
                        apps.push(app);
                    }
                }
                Ok(apps)
            })
            .await
            .map_err(|_| AppError::new("application.discovery_unavailable"))?
        }
        async fn scan_standard_directories(&self) -> Result<Vec<BundleCandidate>, AppError> {
            let cancelled = Arc::new(AtomicBool::new(false));
            let _cancel_on_drop = super::CancelOnDrop::new(cancelled.clone());
            task::spawn_blocking(move || {
                let deadline = Instant::now() + Duration::from_secs(25);
                let mut apps = shallow(Path::new("/Applications"), &cancelled, deadline);
                apps.extend(shallow(
                    Path::new("/System/Applications"),
                    &cancelled,
                    deadline,
                ));
                if let Some(home) = super::home_applications() {
                    apps.extend(shallow(&home, &cancelled, deadline));
                }
                if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
                    Err(AppError::new("application.scan_timeout"))
                } else {
                    Ok(apps)
                }
            })
            .await
            .map_err(|_| not_found())?
        }
        async fn import_selection(
            &self,
            selected: &Path,
        ) -> Result<Vec<BundleCandidate>, AppError> {
            let selected = selected.to_path_buf();
            task::spawn_blocking(move || {
                let Ok(metadata) = fs::symlink_metadata(&selected) else {
                    return Vec::new();
                };
                if metadata.file_type().is_symlink() {
                    return Vec::new();
                }
                if selected
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("app"))
                {
                    inspect(&selected).into_iter().collect()
                } else if metadata.is_dir() {
                    recursively_selected(&selected)
                } else {
                    Vec::new()
                }
            })
            .await
            .map_err(|_| not_found())
        }
    }

    type Object = c_void;
    #[link(name = "objc")]
    extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut Object;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
    }
    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}
    unsafe fn selector(value: &str) -> *mut c_void {
        sel_registerName(CString::new(value).unwrap().as_ptr())
    }
    fn validate_persisted_requirement(
        url: CFURLRef,
        requirement_text: &str,
    ) -> Result<(), AppError> {
        if requirement_text.trim().is_empty() {
            return Err(AppError::application_signature_changed());
        }
        unsafe {
            let text = CString::new(requirement_text)
                .map_err(|_| AppError::application_signature_changed())?;
            let text = CFStringCreateWithCString(kCFAllocatorDefault, text.as_ptr(), UTF8);
            if text.is_null() {
                return Err(AppError::application_signature_changed());
            }
            let mut requirement = ptr::null();
            let compiled = SecRequirementCreateWithString(text, 0, &mut requirement);
            CFRelease(text);
            if compiled != 0 || requirement.is_null() {
                return Err(AppError::application_signature_changed());
            }
            let mut code = ptr::null();
            let created = SecStaticCodeCreateWithPath(url, 0, &mut code);
            if created != 0 || code.is_null() {
                CFRelease(requirement);
                return Err(AppError::application_signature_changed());
            }
            let valid = SecStaticCodeCheckValidity(code, K_SEC_CS_STRICT_VALIDATE, requirement);
            CFRelease(code);
            CFRelease(requirement);
            if valid == 0 {
                Ok(())
            } else {
                Err(AppError::application_signature_changed())
            }
        }
    }

    struct LaunchedProcess {
        process_id: u32,
        running: *mut Object,
        newly_launched: bool,
    }

    fn is_newly_launched(snapshot: Option<&HashSet<u32>>, returned_pid: u32) -> bool {
        snapshot.is_some_and(|pids| !pids.contains(&returned_pid))
    }

    unsafe fn running_application_pids(bundle_id: &str) -> Option<HashSet<u32>> {
        let Ok(bundle_id) = CString::new(bundle_id) else {
            return None;
        };
        let string: unsafe extern "C" fn(*mut Object, *mut c_void, *const c_char) -> *mut Object =
            std::mem::transmute(objc_msgSend as *const ());
        let identifier = string(
            objc_getClass(c"NSString".as_ptr()),
            selector("stringWithUTF8String:"),
            bundle_id.as_ptr(),
        );
        if identifier.is_null() {
            return None;
        }
        let applications: unsafe extern "C" fn(
            *mut Object,
            *mut c_void,
            *mut Object,
        ) -> *mut Object = std::mem::transmute(objc_msgSend as *const ());
        let applications = applications(
            objc_getClass(c"NSRunningApplication".as_ptr()),
            selector("runningApplicationsWithBundleIdentifier:"),
            identifier,
        );
        if applications.is_null() {
            return None;
        }
        let count: unsafe extern "C" fn(*mut Object, *mut c_void) -> usize =
            std::mem::transmute(objc_msgSend as *const ());
        let at: unsafe extern "C" fn(*mut Object, *mut c_void, usize) -> *mut Object =
            std::mem::transmute(objc_msgSend as *const ());
        let pid: unsafe extern "C" fn(*mut Object, *mut c_void) -> i32 =
            std::mem::transmute(objc_msgSend as *const ());
        Some(
            (0..count(applications, selector("count")))
                .filter_map(|index| {
                    let value = pid(
                        at(applications, selector("objectAtIndex:"), index),
                        selector("processIdentifier"),
                    );
                    (value > 0).then_some(value as u32)
                })
                .collect(),
        )
    }

    fn launch_url(url: CFURLRef, expected_bundle_id: &str) -> Result<LaunchedProcess, AppError> {
        unsafe {
            let pool: unsafe extern "C" fn(*mut Object, *mut c_void) -> *mut Object =
                std::mem::transmute(objc_msgSend as *const ());
            let release: unsafe extern "C" fn(*mut Object, *mut c_void) =
                std::mem::transmute(objc_msgSend as *const ());
            let alloc: unsafe extern "C" fn(*mut Object, *mut c_void) -> *mut Object =
                std::mem::transmute(objc_msgSend as *const ());
            let init: unsafe extern "C" fn(*mut Object, *mut c_void) -> *mut Object =
                std::mem::transmute(objc_msgSend as *const ());
            let autpool = pool(
                alloc(
                    objc_getClass(c"NSAutoreleasePool".as_ptr()),
                    selector("alloc"),
                ),
                selector("init"),
            );
            let workspace = pool(
                objc_getClass(c"NSWorkspace".as_ptr()),
                selector("sharedWorkspace"),
            );
            // A pre-launch snapshot prevents an attestation/cancellation failure from killing a
            // user-owned instance that NSWorkspace merely returned instead of starting anew.
            let prior_pids = running_application_pids(expected_bundle_id);
            let dictionary = init(
                alloc(objc_getClass(c"NSDictionary".as_ptr()), selector("alloc")),
                selector("init"),
            );
            let mut error: *mut Object = ptr::null_mut();
            // The method returns NSRunningApplication *, not BOOL.
            let send: unsafe extern "C" fn(
                *mut Object,
                *mut c_void,
                *mut Object,
                usize,
                *mut Object,
                *mut *mut Object,
            ) -> *mut Object = std::mem::transmute(objc_msgSend as *const ());
            let running = send(
                workspace,
                selector("launchApplicationAtURL:options:configuration:error:"),
                url as *mut Object,
                0,
                dictionary,
                &mut error,
            );
            // Retain the running app before draining its autorelease pool; it is terminated/released
            // after post-launch attestation.
            let retain: unsafe extern "C" fn(*mut Object, *mut c_void) -> *mut Object =
                std::mem::transmute(objc_msgSend as *const ());
            let running = if running.is_null() {
                running
            } else {
                retain(running, selector("retain"))
            };
            release(dictionary, selector("release"));
            release(autpool, selector("drain"));
            if running.is_null() {
                Err(not_found())
            } else {
                let pid: unsafe extern "C" fn(*mut Object, *mut c_void) -> i32 =
                    std::mem::transmute(objc_msgSend as *const ());
                let value = pid(running, selector("processIdentifier"));
                if value <= 0 {
                    release(running, selector("release"));
                    Err(AppError::application_signature_changed())
                } else {
                    Ok(LaunchedProcess {
                        process_id: value as u32,
                        running,
                        newly_launched: is_newly_launched(prior_pids.as_ref(), value as u32),
                    })
                }
            }
        }
    }

    fn terminate_running_process(running: *mut Object) -> bool {
        unsafe {
            let terminate: unsafe extern "C" fn(*mut Object, *mut c_void) -> u8 =
                std::mem::transmute(objc_msgSend as *const ());
            let release: unsafe extern "C" fn(*mut Object, *mut c_void) =
                std::mem::transmute(objc_msgSend as *const ());
            let terminated: unsafe extern "C" fn(*mut Object, *mut c_void) -> u8 =
                std::mem::transmute(objc_msgSend as *const ());
            let force_terminate: unsafe extern "C" fn(*mut Object, *mut c_void) -> u8 =
                std::mem::transmute(objc_msgSend as *const ());
            let _graceful = terminate(running, selector("terminate")) != 0;
            let mut stopped = false;
            for _ in 0..50 {
                if terminated(running, selector("isTerminated")) != 0 {
                    stopped = true;
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            if !stopped {
                let _forced = force_terminate(running, selector("forceTerminate")) != 0;
            }
            if !stopped {
                for _ in 0..50 {
                    if terminated(running, selector("isTerminated")) != 0 {
                        stopped = true;
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            }
            release(running, selector("release"));
            stopped
        }
    }
    fn release_running_process(running: *mut Object) {
        unsafe {
            let release: unsafe extern "C" fn(*mut Object, *mut c_void) =
                std::mem::transmute(objc_msgSend as *const ());
            release(running, selector("release"));
        }
    }
    fn discard_failed_launch(process: LaunchedProcess) -> Result<(), AppError> {
        if process.newly_launched {
            if terminate_running_process(process.running) {
                Ok(())
            } else {
                Err(AppError::new("application.termination_failed"))
            }
        } else {
            // NSWorkspace can return a pre-existing app. Reject credential use without touching
            // that user-owned process.
            release_running_process(process.running);
            Ok(())
        }
    }
    pub(super) fn attest_running_process(
        pid: u32,
        requirement_text: &str,
        expected_bundle_id: &str,
    ) -> Result<(), AppError> {
        if requirement_text.trim().is_empty() || expected_bundle_id.trim().is_empty() {
            return Err(AppError::application_signature_changed());
        }
        unsafe {
            let text = CString::new(requirement_text)
                .map_err(|_| AppError::application_signature_changed())?;
            let text = CFStringCreateWithCString(kCFAllocatorDefault, text.as_ptr(), UTF8);
            let mut requirement = ptr::null();
            if text.is_null()
                || SecRequirementCreateWithString(text, 0, &mut requirement) != 0
                || requirement.is_null()
            {
                if !text.is_null() {
                    CFRelease(text)
                }
                return Err(AppError::application_signature_changed());
            }
            CFRelease(text);
            let raw_pid = pid as i32;
            let number = CFNumberCreate(kCFAllocatorDefault, 3, (&raw_pid as *const i32).cast());
            let attributes =
                CFDictionaryCreateMutable(kCFAllocatorDefault, 1, ptr::null(), ptr::null());
            if number.is_null() || attributes.is_null() {
                if !number.is_null() {
                    CFRelease(number)
                }
                if !attributes.is_null() {
                    CFRelease(attributes)
                }
                CFRelease(requirement);
                return Err(AppError::application_signature_changed());
            }
            CFDictionarySetValue(attributes, kSecGuestAttributePid, number);
            let mut guest = ptr::null();
            let copied = SecCodeCopyGuestWithAttributes(ptr::null(), attributes, 0, &mut guest);
            CFRelease(number);
            CFRelease(attributes);
            let mut signing_information = ptr::null();
            let bundle_ok = copied == 0
                && !guest.is_null()
                && SecCodeCopySigningInformation(guest, 0, &mut signing_information) == 0
                && !signing_information.is_null()
                && string(CFDictionaryGetValue(
                    signing_information,
                    kSecCodeInfoIdentifier,
                ))
                .ok()
                .as_deref()
                    == Some(expected_bundle_id);
            if !signing_information.is_null() {
                CFRelease(signing_information);
            }
            let valid = if copied == 0 && !guest.is_null() && bundle_ok {
                SecCodeCheckValidity(guest, K_SEC_CS_STRICT_VALIDATE, requirement)
            } else {
                -1
            };
            if !guest.is_null() {
                CFRelease(guest)
            }
            CFRelease(requirement);
            if valid == 0 {
                Ok(())
            } else {
                Err(AppError::application_signature_changed())
            }
        }
    }

    fn atomic_verify_and_launch(
        app: &ApplicationRecord,
        cancelled: &AtomicBool,
    ) -> Result<LaunchAttestation, AppError> {
        if cancelled.load(Ordering::Acquire) {
            return Err(AppError::new("application.cancelled"));
        }
        let target = stable_application_url(app)?;
        // Metadata and Bundle ID are read through the stable URL. Authorization itself is
        // exclusively the persisted requirement compiled below, never a re-rendered DR string.
        let current = inspect_url(target.url(), target.path(), false)?;
        if !current.user_facing() || current.bundle_id != app.platform_application_id {
            return Err(AppError::application_signature_changed());
        }
        validate_persisted_requirement(target.url(), &app.signature_identity)?;
        // NSWorkspace has returned a process; post-launch attestation is performed below.
        if cancelled.load(Ordering::Acquire) {
            return Err(AppError::new("application.cancelled"));
        }
        let launched = launch_url(target.url(), &app.platform_application_id)?;
        if cancelled.load(Ordering::Acquire) {
            discard_failed_launch(launched)?;
            return Err(AppError::new("application.cancelled"));
        }
        if let Err(error) = attest_running_process(
            launched.process_id,
            &app.signature_identity,
            &app.platform_application_id,
        ) {
            discard_failed_launch(launched)?;
            return Err(error);
        }
        if cancelled.load(Ordering::Acquire) {
            discard_failed_launch(launched)?;
            return Err(AppError::new("application.cancelled"));
        }
        release_running_process(launched.running);
        Ok(LaunchAttestation {
            target: target.path().to_string_lossy().into_owned(),
            process_id: launched.process_id,
        })
    }

    #[async_trait]
    impl AtomicVerifier for NativeAtomicVerifier {
        async fn verify_and_launch(
            &self,
            app: &ApplicationRecord,
            cancelled: Arc<AtomicBool>,
        ) -> Result<LaunchAttestation, AppError> {
            let app = app.clone();
            task::spawn_blocking(move || atomic_verify_and_launch(&app, &cancelled))
                .await
                .map_err(|_| AppError::new("application.launch_unavailable"))?
        }
    }

    #[cfg(test)]
    mod tests {
        use std::{
            collections::HashSet,
            fs,
            os::unix::fs::symlink,
            process::Command,
            time::{Duration, Instant},
        };

        use uuid::Uuid;

        use super::{is_newly_launched, recursively_selected, run_bounded_process, ProcessLimits};

        #[test]
        fn launch_ownership_uses_returned_pid_membership_not_any_existing_instance() {
            let snapshot = HashSet::from([41_u32]);
            assert!(is_newly_launched(Some(&snapshot), 42));
            assert!(!is_newly_launched(Some(&snapshot), 41));
            // A failed snapshot is conservative: avoid killing a process whose ownership is unknown.
            assert!(!is_newly_launched(None, 42));
        }

        fn run_shell(
            script: &str,
            timeout: Duration,
            limits: ProcessLimits,
        ) -> Result<Vec<u8>, ()> {
            let mut command = Command::new("/bin/sh");
            command.arg("-c").arg(script);
            run_bounded_process(command, timeout, limits).map_err(|_| ())
        }

        #[test]
        fn bounded_runner_preserves_nul_delimited_raw_stdout() {
            let output = run_shell(
                "printf '/Applications/One.app\\0/Applications/Two\\377.app\\0'",
                Duration::from_secs(1),
                ProcessLimits::new(1024, 1024, 2048),
            )
            .unwrap();
            assert_eq!(
                output,
                b"/Applications/One.app\0/Applications/Two\xff.app\0"
            );
        }

        #[test]
        fn bounded_runner_kills_the_process_group_on_timeout_without_waiting_for_sleep() {
            let pid_file =
                std::env::temp_dir().join(format!("autologin-indexed-pid-{}", Uuid::new_v4()));
            let script = format!(
                "sleep 30 & child=$!; echo $child > {}; wait",
                pid_file.display()
            );
            let started = Instant::now();
            assert!(run_shell(
                &script,
                Duration::from_millis(100),
                ProcessLimits::new(1024, 1024, 2048),
            )
            .is_err());
            assert!(started.elapsed() < Duration::from_secs(1));

            let child_pid = fs::read_to_string(&pid_file).unwrap();
            let reaped = (0..50).any(|_| {
                let status = Command::new("/bin/ps")
                    .args(["-o", "stat=", "-p", child_pid.trim()])
                    .output()
                    .unwrap();
                if status.stdout.is_empty() {
                    true
                } else {
                    std::thread::sleep(Duration::from_millis(10));
                    false
                }
            });
            assert!(reaped, "timed-out descendant was not reaped/killed");
            fs::remove_file(pid_file).unwrap();
        }

        #[test]
        fn bounded_runner_does_not_block_after_leader_exits_with_a_pipe_holding_descendant() {
            let started = Instant::now();
            assert!(run_shell(
                "sleep 2 & exit 0",
                Duration::from_millis(100),
                ProcessLimits::new(1024, 1024, 2048),
            )
            .is_err());
            assert!(started.elapsed() < Duration::from_millis(500));
        }

        #[test]
        fn bounded_runner_rejects_stdout_before_buffering_past_its_limit() {
            assert!(run_shell(
                "yes x | head -c 4096",
                Duration::from_secs(1),
                ProcessLimits::new(64, 1024, 1088),
            )
            .is_err());
        }

        #[test]
        fn bounded_runner_rejects_stderr_and_nonzero_status() {
            assert!(run_shell(
                "yes x | head -c 4096 >&2",
                Duration::from_secs(1),
                ProcessLimits::new(1024, 64, 1088),
            )
            .is_err());
            assert!(run_shell(
                "exit 17",
                Duration::from_secs(1),
                ProcessLimits::new(1024, 1024, 2048),
            )
            .is_err());
        }

        #[test]
        fn bounded_runner_applies_the_shared_output_limit_across_both_pipes() {
            assert!(run_shell(
                "head -c 96 /dev/zero; head -c 96 /dev/zero >&2",
                Duration::from_secs(1),
                ProcessLimits::new(128, 128, 128),
            )
            .is_err());
        }

        #[test]
        fn explicit_folder_scan_skips_a_symlink_cycle() {
            let root = std::env::temp_dir().join(format!("autologin-cycle-{}", Uuid::new_v4()));
            fs::create_dir_all(root.join("nested")).unwrap();
            symlink(&root, root.join("nested/again")).unwrap();
            assert!(recursively_selected(&root).is_empty());
            fs::remove_dir_all(root).unwrap();
        }
    }
}

/// Revalidate the already-bound process; never launches or chooses a different PID.
#[cfg(target_os = "macos")]
pub(crate) fn revalidate_fill_process(verified: &VerifiedApplication) -> Result<(), AppError> {
    macos::attest_running_process(
        verified.launched_process_id,
        &verified.application.signature_identity,
        &verified.application.platform_application_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use autologin_core::{ApplicationCatalog, ApplicationRecord, DiscoverySource, Platform};
    use std::{
        path::Path,
        sync::{Arc, Mutex},
    };

    #[derive(Default)]
    struct FixtureSource {
        registered: Vec<BundleCandidate>,
        scanned: Vec<BundleCandidate>,
        imported: Vec<BundleCandidate>,
    }
    #[async_trait]
    impl AppSource for FixtureSource {
        async fn registered_apps(&self) -> Result<Vec<BundleCandidate>, AppError> {
            Ok(self.registered.clone())
        }
        async fn scan_standard_directories(&self) -> Result<Vec<BundleCandidate>, AppError> {
            Ok(self.scanned.clone())
        }
        async fn import_selection(&self, _: &Path) -> Result<Vec<BundleCandidate>, AppError> {
            Ok(self.imported.clone())
        }
    }
    #[derive(Default)]
    struct FixtureBookmarks {
        created: Mutex<Vec<PathBuf>>,
    }
    impl BookmarkCreator for FixtureBookmarks {
        fn create(&self, path: &Path) -> Result<String, AppError> {
            self.created.lock().unwrap().push(path.into());
            Ok("opaque-bookmark".into())
        }
    }
    struct FixtureVerifier {
        verification_calls: Mutex<usize>,
        launch_calls: Mutex<usize>,
        result: Result<LaunchAttestation, AppError>,
    }
    impl FixtureVerifier {
        fn success(path: &str) -> Self {
            Self {
                verification_calls: Mutex::new(0),
                launch_calls: Mutex::new(0),
                result: Ok(LaunchAttestation {
                    target: path.into(),
                    process_id: 4242,
                }),
            }
        }
        fn failure(code: &str) -> Self {
            Self {
                verification_calls: Mutex::new(0),
                launch_calls: Mutex::new(0),
                result: Err(AppError::new(code)),
            }
        }
    }
    #[async_trait]
    impl AtomicVerifier for FixtureVerifier {
        async fn verify_and_launch(
            &self,
            _: &ApplicationRecord,
            _: Arc<AtomicBool>,
        ) -> Result<LaunchAttestation, AppError> {
            *self.verification_calls.lock().unwrap() += 1;
            if self.result.is_ok() {
                *self.launch_calls.lock().unwrap() += 1;
            }
            self.result.clone()
        }
    }
    fn app(path: &str, id: &str) -> BundleCandidate {
        BundleCandidate {
            path: path.into(),
            display_name: Path::new(path)
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .into(),
            bundle_id: id.into(),
            version: Some("1".into()),
            signature_identity: "anchor apple generic and identifier \"com.example.Chat\"".into(),
            package_type: "APPL".into(),
            activation_policy: ActivationPolicy::Regular,
            has_executable: true,
        }
    }
    fn fixture_source() -> FixtureSource {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/apps/duplicates.json")).unwrap();
        let parse = |value: &serde_json::Value| BundleCandidate {
            path: value["path"].as_str().unwrap().into(),
            display_name: value["display_name"].as_str().unwrap().into(),
            bundle_id: value["bundle_id"].as_str().unwrap().into(),
            version: None,
            signature_identity: value["signature_identity"].as_str().unwrap().into(),
            package_type: "APPL".into(),
            activation_policy: ActivationPolicy::Regular,
            has_executable: true,
        };
        FixtureSource {
            registered: fixture["registered"]
                .as_array()
                .unwrap()
                .iter()
                .map(&parse)
                .collect(),
            scanned: fixture["scanned"]
                .as_array()
                .unwrap()
                .iter()
                .map(parse)
                .collect(),
            ..Default::default()
        }
    }

    fn catalog(
        source: FixtureSource,
        bookmarks: Arc<FixtureBookmarks>,
        verifier: Arc<FixtureVerifier>,
    ) -> MacApplicationCatalog {
        MacApplicationCatalog::from_parts(Arc::new(source), bookmarks, verifier)
    }

    #[tokio::test]
    async fn discovery_deduplicates_bundle_identifier_and_retains_alternate() {
        let source = fixture_source();
        let apps = catalog(
            source,
            Arc::new(FixtureBookmarks::default()),
            Arc::new(FixtureVerifier::success("/stable/Chat.app")),
        )
        .discover()
        .await
        .unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].launch_target, "/Applications/Chat.app");
        assert_eq!(
            apps[0].alternate_launch_targets,
            vec!["/Applications/ZChat.app"]
        );
    }

    #[tokio::test]
    async fn discovery_filters_helpers_and_background_bundles() {
        let mut background = app("/Applications/Agent.app", "agent");
        background.activation_policy = ActivationPolicy::Prohibited;
        let source = FixtureSource {
            registered: vec![
                app("/Applications/Chat.app", "chat"),
                app("/Applications/Chat Helper.app", "helper"),
            ],
            scanned: vec![background],
            ..Default::default()
        };
        let apps = catalog(
            source,
            Arc::new(FixtureBookmarks::default()),
            Arc::new(FixtureVerifier::success("/stable/Chat.app")),
        )
        .discover()
        .await
        .unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].platform_application_id, "chat");
    }

    #[tokio::test]
    async fn automatic_discovery_excludes_system_library_components() {
        let source = FixtureSource {
            registered: vec![
                app(
                    "/System/Library/Services/ChineseTextConverterService.app",
                    "com.apple.ChineseTextConverterService",
                ),
                app(
                    "/System/Library/Input Methods/EmojiFunctionRowIM.app",
                    "com.apple.EmojiFunctionRowIM",
                ),
            ],
            ..Default::default()
        };
        let apps = catalog(
            source,
            Arc::new(FixtureBookmarks::default()),
            Arc::new(FixtureVerifier::success("/stable")),
        )
        .discover()
        .await
        .unwrap();

        assert!(apps.is_empty());
    }

    #[tokio::test]
    #[cfg(target_os = "macos")]
    async fn automatic_discovery_keeps_only_user_installed_application_roots() {
        let home_app = home_applications().unwrap().join("Custom.app");
        let source = FixtureSource {
            registered: vec![
                app("/Applications/WeChat.app", "com.tencent.xinWeChat"),
                app("/Applications/QQ.app", "com.tencent.qq"),
                app("/Applications/Steam.app", "com.valvesoftware.steam"),
                app(home_app.to_str().unwrap(), "com.example.custom"),
                app(
                    "/System/Applications/Utilities/Terminal.app",
                    "com.apple.Terminal",
                ),
            ],
            ..Default::default()
        };
        let apps = catalog(
            source,
            Arc::new(FixtureBookmarks::default()),
            Arc::new(FixtureVerifier::success("/stable")),
        )
        .discover()
        .await
        .unwrap();
        let ids: Vec<_> = apps
            .iter()
            .map(|application| application.platform_application_id.as_str())
            .collect();

        assert_eq!(
            ids,
            vec![
                "com.example.custom",
                "com.tencent.qq",
                "com.tencent.xinWeChat",
                "com.valvesoftware.steam",
            ]
        );
    }

    #[tokio::test]
    async fn manual_import_cannot_promote_filtered_helpers_to_launchable_records() {
        let bookmarks = Arc::new(FixtureBookmarks::default());
        let source = FixtureSource {
            imported: vec![app("/tmp/Chat Helper.app", "helper")],
            ..Default::default()
        };
        let error = catalog(
            source,
            bookmarks.clone(),
            Arc::new(FixtureVerifier::success("/tmp/Chat Helper.app")),
        )
        .import_bundle(Path::new("/tmp/Chat Helper.app"))
        .await
        .unwrap_err();
        assert_eq!(error.code(), "application.unsupported_import");
        assert!(bookmarks.created.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn manual_import_creates_opaque_bookmark_for_nonstandard_bundle() {
        let source = FixtureSource {
            imported: vec![app("/Volumes/External/Chat.app", "chat")],
            ..Default::default()
        };
        let bookmarks = Arc::new(FixtureBookmarks::default());
        let apps = catalog(
            source,
            bookmarks.clone(),
            Arc::new(FixtureVerifier::success("/stable/Chat.app")),
        )
        .import_bundle(Path::new("/Volumes/External"))
        .await
        .unwrap();
        assert_eq!(apps[0].path_access_ref.as_deref(), Some("opaque-bookmark"));
        assert_eq!(bookmarks.created.lock().unwrap().len(), 1);
        assert_eq!(apps[0].discovery_source, DiscoverySource::ManualImport);
    }

    #[tokio::test]
    async fn manual_import_rejects_a_folder_without_a_valid_application_bundle() {
        let mut invalid = app("/tmp/Tool.app", "tool");
        invalid.has_executable = false;
        let source = FixtureSource {
            imported: vec![invalid],
            ..Default::default()
        };
        let error = catalog(
            source,
            Arc::new(FixtureBookmarks::default()),
            Arc::new(FixtureVerifier::success("/stable/Chat.app")),
        )
        .import_bundle(Path::new("/tmp"))
        .await
        .unwrap_err();
        assert_eq!(error.code(), UNSUPPORTED_IMPORT);
    }

    #[test]
    fn support_filter_uses_bundle_filename_not_an_absolute_path_substring() {
        let candidate = app("/Users/agent/Documents/Chat.app", "chat");
        assert!(candidate.user_facing());
        let helper = app("/Applications/Chat Helper.app", "helper");
        assert!(!helper.user_facing());
    }

    #[test]
    fn support_filter_recognizes_exact_and_camel_case_names_without_hiding_regular_apps() {
        for path in [
            "/Applications/Helper.app",
            "/Applications/Updater.app",
            "/Applications/FooHelper.app",
            "/Applications/Chat Helper.app",
        ] {
            assert!(!app(path, "support").user_facing(), "{path}");
        }
        assert!(app("/Applications/LegitimateAgentName.app", "regular").user_facing());
    }

    #[test]
    #[cfg(unix)]
    fn dedupe_collapses_canonical_symlink_aliases() {
        use std::{fs, os::unix::fs::symlink};
        use uuid::Uuid;
        let root = std::env::temp_dir().join(format!("autologin-dedupe-{}", Uuid::new_v4()));
        let canonical = root.join("Chat.app");
        let alias = root.join("Chat Copy.app");
        fs::create_dir_all(&canonical).unwrap();
        symlink(&canonical, &alias).unwrap();
        let records = deduplicate(vec![
            app(canonical.to_str().unwrap(), "chat"),
            app(alias.to_str().unwrap(), "chat"),
        ]);
        assert_eq!(records.len(), 1);
        assert!(records[0].1.is_empty());
        fs::remove_file(alias).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_metadata_fixture_is_not_user_facing() {
        let mut malformed = app("/Applications/Chat.app", "chat");
        malformed.package_type = "CFArray".into();
        assert!(!malformed.user_facing());
    }

    #[tokio::test]
    async fn missing_bookmark_resolution_never_reaches_launch() {
        let verifier = Arc::new(FixtureVerifier::failure("application.not_found"));
        let catalog = catalog(
            FixtureSource::default(),
            Arc::new(FixtureBookmarks::default()),
            verifier.clone(),
        );
        let mut record =
            ApplicationRecord::new(Platform::Macos, "chat".into(), "Chat".into()).unwrap();
        record.path_access_ref = Some("stale-bookmark".into());
        record.signature_identity = "identity".into();
        let error = catalog.verify_and_launch(&record).await.unwrap_err();
        assert_eq!(error.code(), "application.not_found");
        assert_eq!(*verifier.verification_calls.lock().unwrap(), 1);
        assert_eq!(*verifier.launch_calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn signature_change_prevents_atomic_launch() {
        let verifier = Arc::new(FixtureVerifier::failure("application.signature_changed"));
        let catalog = catalog(
            FixtureSource::default(),
            Arc::new(FixtureBookmarks::default()),
            verifier.clone(),
        );
        let record = ApplicationRecord::new(Platform::Macos, "chat".into(), "Chat".into()).unwrap();
        let error = catalog.verify_and_launch(&record).await.unwrap_err();
        assert_eq!(error.code(), "application.signature_changed");
        assert_eq!(*verifier.launch_calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn atomic_verifier_launches_the_stable_target_not_the_mutable_path() {
        let verifier = Arc::new(FixtureVerifier::success("/stable-object/Chat.app"));
        let catalog = catalog(
            FixtureSource::default(),
            Arc::new(FixtureBookmarks::default()),
            verifier.clone(),
        );
        let mut record =
            ApplicationRecord::new(Platform::Macos, "chat".into(), "Chat".into()).unwrap();
        record.launch_target = "/mutable-path/Chat.app".into();
        record.signature_identity = "designated-requirement".into();
        let verified = catalog.verify_and_launch(&record).await.unwrap();
        assert_eq!(verified.verified_launch_target, "/stable-object/Chat.app");
        assert_eq!(verified.launched_process_id, 4242);
        assert_eq!(*verifier.launch_calls.lock().unwrap(), 1);
    }
    #[test]
    fn cancellation_guard_sets_flag_on_drop_and_disarms_on_success() {
        let flag = Arc::new(AtomicBool::new(false));
        {
            let guard = CancelOnDrop::new(flag.clone());
            guard.disarm();
        }
        assert!(!flag.load(Ordering::Acquire));
        {
            let _guard = CancelOnDrop::new(flag.clone());
        }
        assert!(flag.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn indexed_failure_still_returns_standard_scan_candidates() {
        struct FailingIndex;
        #[async_trait]
        impl AppSource for FailingIndex {
            async fn registered_apps(&self) -> Result<Vec<BundleCandidate>, AppError> {
                Err(AppError::new("application.discovery_unavailable"))
            }
            async fn scan_standard_directories(&self) -> Result<Vec<BundleCandidate>, AppError> {
                Ok(vec![app("/Applications/Chat.app", "chat")])
            }
            async fn import_selection(&self, _: &Path) -> Result<Vec<BundleCandidate>, AppError> {
                Ok(Vec::new())
            }
        }
        let catalog = MacApplicationCatalog::from_parts(
            Arc::new(FailingIndex),
            Arc::new(FixtureBookmarks::default()),
            Arc::new(FixtureVerifier::success("/stable")),
        );
        assert_eq!(catalog.discover().await.unwrap().len(), 1);
    }
}

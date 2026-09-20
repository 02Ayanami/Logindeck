use crate::commands::CommandError;
use autologin_core::{
    AppError, ApplicationId, ApplicationRecord, ApplicationsService, DiscoveredApplication,
    Platform,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Notify;

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanPhase {
    Idle,
    Scanning,
    Choosing,
    Cancelling,
    Committing,
    Completed,
    Cancelled,
    Failed,
}
impl ScanPhase {
    fn active(self) -> bool {
        matches!(
            self,
            Self::Scanning | Self::Choosing | Self::Cancelling | Self::Committing
        )
    }
}
#[derive(Clone, Serialize)]
pub struct ScanCandidate {
    pub token: usize,
    pub display_name: String,
    pub selected: bool,
}
#[derive(Clone, Serialize)]
pub struct ScanSnapshot {
    pub id: u64,
    pub phase: ScanPhase,
    pub started_at: Option<u64>,
    pub count: Option<usize>,
    pub error: Option<CommandError>,
    pub candidates: Vec<ScanCandidate>,
}
struct Inner {
    snapshot: ScanSnapshot,
    cancel: Arc<Notify>,
    // Only the backend holds identities, paths, signatures, and stored application IDs.
    entries: Vec<CandidateEntry>,
}
struct CandidateEntry {
    display_name: String,
    icon_path: Option<std::path::PathBuf>,
    discovered: Option<DiscoveredApplication>,
    existing_ids: Vec<ApplicationId>,
}
#[derive(Clone)]
pub struct ScanManager(Arc<Mutex<Inner>>);
impl Default for ScanManager {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Inner {
            snapshot: ScanSnapshot {
                id: 0,
                phase: ScanPhase::Idle,
                started_at: None,
                count: None,
                error: None,
                candidates: vec![],
            },
            cancel: Arc::new(Notify::new()),
            entries: vec![],
        })))
    }
}
fn identity(platform: Platform, bundle: &str) -> (u8, String) {
    (
        match platform {
            Platform::Macos => 0,
            Platform::Windows => 1,
        },
        bundle.to_owned(),
    )
}
fn normalize(
    mut found: Vec<DiscoveredApplication>,
) -> Result<Vec<DiscoveredApplication>, AppError> {
    if found.len() > 4096 {
        return Err(AppError::new("application.discovery_unavailable"));
    }
    for app in &found {
        autologin_core::validate_application_metadata(
            &app.display_name,
            &app.platform_application_id,
            &app.launch_target,
            &app.alternate_launch_targets,
            app.version.as_deref(),
            &app.signature_identity,
            app.path_access_ref.as_deref(),
        )?;
    }
    found.sort_by_key(|app| {
        (
            identity(app.platform, &app.platform_application_id),
            app.launch_target.clone(),
        )
    });
    let mut unique: Vec<DiscoveredApplication> = vec![];
    for app in found {
        if let Some(previous) = unique.last_mut().filter(|previous| {
            identity(previous.platform, &previous.platform_application_id)
                == identity(app.platform, &app.platform_application_id)
        }) {
            previous.alternate_launch_targets.push(app.launch_target);
            previous
                .alternate_launch_targets
                .extend(app.alternate_launch_targets);
            previous.alternate_launch_targets.sort();
            previous.alternate_launch_targets.dedup();
            previous
                .alternate_launch_targets
                .retain(|path| path != &previous.launch_target);
            previous.alternate_launch_targets.truncate(32);
        } else {
            unique.push(app);
        }
    }
    unique.sort_by_key(|app| {
        (
            app.display_name.to_lowercase(),
            app.platform_application_id.clone(),
        )
    });
    Ok(unique)
}
fn prepare_candidates(
    found: Vec<DiscoveredApplication>,
    existing: Vec<ApplicationRecord>,
) -> Vec<CandidateEntry> {
    let mut existing_by_identity = BTreeMap::<(u8, String), Vec<ApplicationRecord>>::new();
    for record in existing {
        existing_by_identity
            .entry(identity(record.platform, &record.platform_application_id))
            .or_default()
            .push(record);
    }
    let mut entries = Vec::with_capacity(found.len() + existing_by_identity.len());
    for app in found {
        let existing_ids = existing_by_identity
            .remove(&identity(app.platform, &app.platform_application_id))
            .unwrap_or_default()
            .into_iter()
            .map(|record| record.id)
            .collect();
        entries.push(CandidateEntry {
            display_name: app.display_name.clone(),
            icon_path: platform_runtime::application_icon_path(app.platform, &app.launch_target),
            discovered: Some(app),
            existing_ids,
        });
    }
    for records in existing_by_identity.into_values() {
        let Some(first) = records.first() else {
            continue;
        };
        entries.push(CandidateEntry {
            display_name: first.display_name.clone(),
            icon_path: platform_runtime::application_icon_path(
                first.platform,
                &first.launch_target,
            ),
            discovered: None,
            existing_ids: records.into_iter().map(|record| record.id).collect(),
        });
    }
    entries.sort_by_key(|entry| entry.display_name.to_lowercase());
    entries
}
impl ScanManager {
    pub fn snapshot(&self) -> ScanSnapshot {
        self.0.lock().unwrap().snapshot.clone()
    }
    pub fn candidate_path(
        &self,
        id: u64,
        token: usize,
    ) -> Result<Option<std::path::PathBuf>, AppError> {
        let inner = self.0.lock().unwrap();
        if inner.snapshot.id != id || inner.snapshot.phase != ScanPhase::Choosing {
            return Err(AppError::new("storage.conflict"));
        }
        inner
            .entries
            .get(token)
            .map(|entry| entry.icon_path.clone())
            .ok_or_else(|| AppError::new("storage.conflict"))
    }
    pub fn cancel(&self, id: u64) -> ScanSnapshot {
        let mut inner = self.0.lock().unwrap();
        if inner.snapshot.id == id {
            match inner.snapshot.phase {
                ScanPhase::Scanning => {
                    inner.snapshot.phase = ScanPhase::Cancelling;
                    inner.cancel.notify_one();
                }
                ScanPhase::Choosing => {
                    inner.snapshot.phase = ScanPhase::Cancelled;
                    inner.snapshot.candidates.clear();
                    inner.entries.clear();
                }
                _ => {}
            }
        }
        inner.snapshot.clone()
    }
    pub fn start(&self, service: ApplicationsService) -> ScanSnapshot {
        self.start_work(
            async move {
                let found = normalize(service.discover_applications().await?)?;
                let existing = service.list_applications().await?;
                Ok((found, existing))
            },
            Duration::from_secs(30),
        )
    }
    fn start_work<D>(&self, discover: D, timeout: Duration) -> ScanSnapshot
    where
        D: Future<Output = Result<(Vec<DiscoveredApplication>, Vec<ApplicationRecord>), AppError>>
            + Send
            + 'static,
    {
        let (snapshot, cancel) = {
            let mut inner = self.0.lock().unwrap();
            if inner.snapshot.phase.active() {
                return inner.snapshot.clone();
            }
            inner.snapshot = ScanSnapshot {
                id: inner.snapshot.id + 1,
                phase: ScanPhase::Scanning,
                started_at: Some(
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                ),
                count: None,
                error: None,
                candidates: vec![],
            };
            inner.entries.clear();
            inner.cancel = Arc::new(Notify::new());
            (inner.snapshot.clone(), inner.cancel.clone())
        };
        let owner = self.clone();
        let id = snapshot.id;
        tauri::async_runtime::spawn(async move {
            let result = tokio::select! {
                biased;
                _ = cancel.notified() => Err(AppError::new("application.cancelled")),
                result = tokio::time::timeout(timeout, discover) => result.unwrap_or_else(|_| Err(AppError::new("application.scan_timeout"))),
            };
            let mut inner = owner.0.lock().unwrap();
            if inner.snapshot.id != id {
                return;
            }
            if inner.snapshot.phase == ScanPhase::Cancelling {
                inner.snapshot.phase = ScanPhase::Cancelled;
                return;
            }
            match result {
                Ok((found, existing)) => {
                    let entries: Vec<_> = prepare_candidates(found, existing)
                        .into_iter()
                        .filter(|entry| entry.existing_ids.is_empty())
                        .collect();
                    inner.snapshot.candidates = entries
                        .iter()
                        .enumerate()
                        .map(|(token, entry)| ScanCandidate {
                            token,
                            display_name: entry.display_name.clone(),
                            selected: !entry.existing_ids.is_empty(),
                        })
                        .collect();
                    inner.snapshot.count = Some(entries.len());
                    inner.entries = entries;
                    // Discovery is read-only. Only an explicit selection can enter Committing.
                    inner.snapshot.phase = ScanPhase::Choosing;
                }
                Err(error) if error.code() == "application.cancelled" => {
                    inner.snapshot.phase = ScanPhase::Cancelled
                }
                Err(error) => {
                    inner.snapshot.phase = ScanPhase::Failed;
                    inner.snapshot.error = Some(error.into());
                }
            }
        });
        snapshot
    }
    fn take_selection(
        &self,
        id: u64,
        tokens: Vec<usize>,
    ) -> Result<
        (
            ScanSnapshot,
            Vec<DiscoveredApplication>,
            Vec<ApplicationId>,
            usize,
        ),
        AppError,
    > {
        let mut inner = self.0.lock().unwrap();
        if inner.snapshot.id != id || inner.snapshot.phase != ScanPhase::Choosing {
            return Err(AppError::new("storage.conflict"));
        }
        let selected: BTreeSet<_> = tokens.iter().copied().collect();
        if selected.len() != tokens.len()
            || selected
                .iter()
                .any(|&index| inner.snapshot.candidates.get(index).is_none())
        {
            return Err(AppError::new("storage.conflict"));
        }
        let discovered = inner
            .entries
            .iter()
            .enumerate()
            .filter(|(index, _)| selected.contains(index))
            .filter_map(|(_, entry)| entry.discovered.clone())
            .collect();
        let removed = inner
            .entries
            .iter()
            .enumerate()
            .filter(|(index, _)| !selected.contains(index))
            .flat_map(|(_, entry)| entry.existing_ids.iter().copied())
            .collect();
        let selected_count = selected.len();
        inner.snapshot.phase = ScanPhase::Committing;
        inner.snapshot.candidates.clear();
        inner.entries.clear();
        Ok((inner.snapshot.clone(), discovered, removed, selected_count))
    }
    pub fn confirm(
        &self,
        id: u64,
        tokens: Vec<usize>,
        service: ApplicationsService,
    ) -> Result<ScanSnapshot, AppError> {
        let (snapshot, selected, removed, selected_count) = self.take_selection(id, tokens)?;
        let owner = self.clone();
        // The task owns the commit even if the page closes. Never replay an uncertain write.
        tauri::async_runtime::spawn(async move {
            let _ = removed;
            let result = service.add_discovered_applications(selected).await;
            let mut inner = owner.0.lock().unwrap();
            if inner.snapshot.id != id {
                return;
            }
            match result {
                Ok(_) => {
                    inner.snapshot.phase = ScanPhase::Completed;
                    inner.snapshot.count = Some(selected_count);
                }
                Err(error) => {
                    inner.snapshot.phase = ScanPhase::Failed;
                    inner.snapshot.error = Some(error.into());
                }
            }
        });
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_identities_have_a_stable_exhaustive_order() {
        assert_eq!(
            identity(Platform::Macos, "same.app"),
            (0, "same.app".into())
        );
        assert_eq!(
            identity(Platform::Windows, "same.app"),
            (1, "same.app".into())
        );
    }

    fn candidate(bundle: &str) -> DiscoveredApplication {
        DiscoveredApplication {
            platform: Platform::Macos,
            display_name: bundle.into(),
            platform_application_id: bundle.into(),
            launch_target: format!("/Applications/{bundle}.app"),
            alternate_launch_targets: vec![],
            path_access_ref: None,
            version: None,
            signature_identity: "fixture".into(),
            discovery_source: autologin_core::DiscoverySource::Automatic,
        }
    }
    async fn settled(manager: &ScanManager) -> ScanSnapshot {
        for _ in 0..200 {
            let state = manager.snapshot();
            if !matches!(
                state.phase,
                ScanPhase::Scanning | ScanPhase::Cancelling | ScanPhase::Committing
            ) {
                return state;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("scan did not settle");
    }
    struct Catalog(Vec<DiscoveredApplication>);
    #[async_trait::async_trait]
    impl autologin_core::ApplicationCatalog for Catalog {
        async fn discover(&self) -> Result<Vec<DiscoveredApplication>, AppError> {
            Ok(self.0.clone())
        }
        async fn import_bundle(
            &self,
            _: &std::path::Path,
        ) -> Result<Vec<DiscoveredApplication>, AppError> {
            panic!("selection must not import frontend paths")
        }
        async fn verify_and_launch(
            &self,
            _: &autologin_core::ApplicationRecord,
        ) -> Result<autologin_core::VerifiedApplication, AppError> {
            panic!("discovery must not launch apps")
        }
    }
    #[test]
    fn scan_and_cancel_write_nothing_and_confirmation_adds_only_selected_apps() {
        tauri::async_runtime::block_on(async {
            let repos = autologin_core::SqliteRepositories::connect("sqlite::memory:")
                .await
                .unwrap();
            repos.migrate().await.unwrap();
            let service = ApplicationsService::new(
                repos.applications(),
                repos.application_accounts(),
                Arc::new(Catalog(vec![
                    candidate("a.new"),
                    candidate("b.new"),
                    candidate("c.saved"),
                ])),
            );
            let old = service
                .add_discovered_applications(vec![candidate("c.saved")])
                .await
                .unwrap()
                .remove(0);
            let manager = ScanManager::default();
            let first = manager.start(service.clone());
            let choices = settled(&manager).await;
            assert!(choices.phase == ScanPhase::Choosing);
            assert_eq!(
                repos.applications().list().await.unwrap(),
                vec![old.clone()]
            );
            manager.cancel(first.id);
            assert_eq!(
                repos.applications().list().await.unwrap(),
                vec![old.clone()]
            );
            let second = manager.start(service.clone());
            let choices = settled(&manager).await;
            assert_eq!(choices.candidates.len(), 2);
            assert!(choices
                .candidates
                .iter()
                .all(|candidate| { candidate.display_name != "c.saved" && !candidate.selected }));
            assert!(choices
                .candidates
                .iter()
                .filter(|candidate| candidate.display_name != "c.saved")
                .all(|candidate| !candidate.selected));
            let chosen = choices
                .candidates
                .iter()
                .find(|c| c.display_name == "b.new")
                .unwrap()
                .token;
            manager.confirm(second.id, vec![chosen], service).unwrap();
            let done = settled(&manager).await;
            assert!(done.phase == ScanPhase::Completed);
            assert_eq!(done.count, Some(1));
            let apps = repos.applications().list().await.unwrap();
            assert_eq!(apps.len(), 2);
            assert!(apps.iter().any(|a| a.platform_application_id == "b.new"));
            assert!(!apps.iter().any(|a| a.platform_application_id == "a.new"));
            assert!(repos.applications().get(old.id).await.unwrap().is_some());
        });
    }
    #[test]
    #[cfg(target_os = "windows")]
    fn windows_scan_icon_paths_keep_executables_and_never_convert_aumids_to_paths() {
        tauri::async_runtime::block_on(async {
            let mut executable = candidate("Editor");
            executable.platform = Platform::Windows;
            executable.launch_target = r"exe:C:\Apps\Editor.exe".into();
            let mut packaged = candidate("Package");
            packaged.platform = Platform::Windows;
            packaged.launch_target = "aumid:Contoso.Chat_abc!App".into();
            let manager = ScanManager::default();
            let initial = manager.start_work(
                async { Ok((vec![executable, packaged], vec![])) },
                Duration::from_secs(1),
            );
            assert!(settled(&manager).await.phase == ScanPhase::Choosing);
            assert_eq!(
                manager.candidate_path(initial.id, 0).unwrap(),
                Some(std::path::PathBuf::from(r"C:\Apps\Editor.exe"))
            );
            assert_eq!(manager.candidate_path(initial.id, 1).unwrap(), None);
            assert!(manager.candidate_path(initial.id, 2).is_err());
        });
    }

    #[test]
    fn discovery_waits_for_selection_and_rejects_stale_duplicate_and_invalid_tokens() {
        tauri::async_runtime::block_on(async {
            let manager = ScanManager::default();
            let initial = manager.start_work(
                async { Ok((vec![candidate("new"), candidate("saved")], vec![])) },
                Duration::from_secs(1),
            );
            assert!(settled(&manager).await.phase == ScanPhase::Choosing);
            assert_eq!(manager.snapshot().candidates.len(), 2);
            #[cfg(target_os = "macos")]
            assert_eq!(
                manager.candidate_path(initial.id, 0).unwrap(),
                Some(std::path::PathBuf::from("/Applications/new.app"))
            );
            #[cfg(target_os = "windows")]
            assert_eq!(manager.candidate_path(initial.id, 0).unwrap(), None);
            assert!(manager.candidate_path(initial.id + 1, 0).is_err());
            let duplicate =
                manager.start_work(async { Ok((vec![], vec![])) }, Duration::from_secs(1));
            assert_eq!(duplicate.id, initial.id);
            for (id, tokens) in [
                (initial.id + 1, vec![0]),
                (initial.id, vec![99]),
                (initial.id, vec![0, 0]),
                (initial.id, vec![2]),
            ] {
                assert!(manager.take_selection(id, tokens).is_err());
                assert!(manager.snapshot().phase == ScanPhase::Choosing);
            }
            let (_, selected, removed, count) =
                manager.take_selection(initial.id, vec![0]).unwrap();
            assert_eq!(selected.len(), 1);
            assert_eq!(selected[0].platform_application_id, "new");
            assert!(removed.is_empty());
            assert_eq!(count, 1);
            assert!(manager.take_selection(initial.id, vec![0]).is_err());
        });
    }
    #[test]
    fn cancellation_discards_candidates_and_timeout_releases_scan() {
        tauri::async_runtime::block_on(async {
            let manager = ScanManager::default();
            let first = manager.start_work(std::future::pending(), Duration::from_secs(1));
            manager.cancel(first.id);
            assert!(settled(&manager).await.phase == ScanPhase::Cancelled);
            let second = manager.start_work(
                async { Ok((vec![candidate("new")], vec![])) },
                Duration::from_secs(1),
            );
            manager.cancel(first.id);
            assert!(settled(&manager).await.phase == ScanPhase::Choosing);
            manager.cancel(second.id);
            assert!(manager.snapshot().candidates.is_empty());
            assert!(manager.take_selection(second.id, vec![0]).is_err());
            manager.start_work(std::future::pending(), Duration::from_millis(10));
            assert_eq!(
                settled(&manager).await.error.unwrap().code,
                "application.scan_timeout"
            );
        });
    }
    #[test]
    fn duplicate_bundles_become_one_candidate_with_alternate_locations() {
        let first = candidate("com.example.app");
        let mut second = first.clone();
        second.launch_target = "/Users/fixture/Applications/Example.app".into();
        let found = normalize(vec![second.clone(), first.clone()]).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].launch_target, first.launch_target);
        assert_eq!(
            found[0].alternate_launch_targets,
            vec![second.launch_target]
        );
    }
}

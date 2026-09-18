//! Local configuration storage. Imported data is validated before persistence and use.
use autologin_core::{
    switcher::{Definition, FlowKind, Outcome, Step},
    ApplicationRecord,
};
use serde::Serialize;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct AdapterStore(pub PathBuf);
#[derive(Clone, Serialize)]
pub struct AdapterSummary {
    pub id: Option<String>,
    pub ready: bool,
    pub error: Option<String>,
}
#[derive(Serialize)]
pub struct AdapterPreview {
    pub source: String,
    pub summary: AdapterSummary,
}
#[derive(Clone)]
pub struct SwitchPlan {
    pub definition: Definition,
    pub logout: String,
    pub login: String,
}
impl SwitchPlan {
    pub fn new(definition: Definition) -> Result<Self, String> {
        use autologin_core::adapter::CredentialField;
        let logout: Vec<_> = definition
            .flows
            .iter()
            .filter(|(_, f)| f.kind == FlowKind::Logout && f.outcome == Outcome::LoggedOut)
            .map(|(id, _)| id.clone())
            .collect();
        // The desktop executes one reviewed login path. A successful result must be
        // observed after the single submit action; filling alone is not a switch.
        let login: Vec<_> = definition
            .flows
            .iter()
            .filter(|(_, f)| {
                if f.kind != FlowKind::Login || f.outcome != Outcome::LoggedInObserved {
                    return false;
                }
                let (state, tail) = match f.steps.as_slice() {
                    [Step::Fill {
                        state: a,
                        field: CredentialField::Username,
                        ..
                    }, Step::Fill {
                        state: b,
                        field: CredentialField::Password,
                        ..
                    }, tail @ ..]
                        if a == b =>
                    {
                        (a, tail)
                    }
                    [Step::Clear {
                        state: a,
                        field: CredentialField::Username,
                        ..
                    }, Step::Clear {
                        state: b,
                        field: CredentialField::Password,
                        ..
                    }, Step::Fill {
                        state: c,
                        field: CredentialField::Username,
                        ..
                    }, Step::Fill {
                        state: d,
                        field: CredentialField::Password,
                        ..
                    }, tail @ ..]
                        if a == b && b == c && c == d =>
                    {
                        (a, tail)
                    }
                    _ => return false,
                };
                let submit_at = tail
                    .iter()
                    .position(|step| matches!(step, Step::Submit { .. }));
                let Some(submit_at) = submit_at else {
                    return false;
                };
                if !tail[..submit_at].iter().all(|step| {
                    let Step::Check {
                        state: check_state,
                        target,
                    } = step
                    else {
                        return false;
                    };
                    check_state == state
                        && definition.states[state]
                            .controls
                            .get(target)
                            .is_some_and(|selector| {
                                selector.role() == autologin_core::adapter::Role::Checkbox
                            })
                }) {
                    return false;
                }
                match &tail[submit_at..] {
                    [Step::Submit {
                        state: submit_state,
                        next_states,
                        ..
                    }, Step::WaitState { state: observed }] => {
                        submit_state == state
                            && next_states.len() == 1
                            && next_states[0] == *observed
                    }
                    [Step::Submit {
                        state: submit_state,
                        next_states,
                        ..
                    }, Step::Challenge {
                        state: challenge,
                        resume_state,
                    }, Step::WaitState { state: observed }] => {
                        submit_state == state
                            && next_states.iter().any(|next| next == challenge)
                            && resume_state == observed
                    }
                    _ => false,
                }
            })
            .map(|(id, _)| id.clone())
            .collect();
        if logout.len() != 1 || login.len() != 1 {
            return Err("unsupported".into());
        }
        let Some(Step::WaitState {
            state: after_logout,
        }) = definition.flows[&logout[0]].steps.last()
        else {
            return Err("unsupported".into());
        };
        let before_login = match &definition.flows[&login[0]].steps[0] {
            Step::Fill { state, .. } | Step::Clear { state, .. } => state,
            _ => return Err("unsupported".into()),
        };
        if after_logout != before_login {
            return Err("unsupported".into());
        }
        Ok(Self {
            definition,
            logout: logout[0].clone(),
            login: login[0].clone(),
        })
    }
}
fn parse(app: &ApplicationRecord, source: &str) -> Result<Definition, String> {
    let definition = Definition::parse(source).map_err(|_| "invalid_adapter")?;
    if definition.platform != app.platform
        || definition.application_id != app.platform_application_id
    {
        return Err("adapter_mismatch".into());
    }
    Ok(definition)
}
fn read(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|_| "unavailable")?;
    if !file.metadata().map_err(|_| "unavailable")?.is_file() {
        return Err("invalid_adapter".into());
    }
    let mut source = String::new();
    file.take(65537)
        .read_to_string(&mut source)
        .map_err(|_| "invalid_adapter")?;
    if source.len() > 65536 {
        return Err("invalid_adapter".into());
    }
    Ok(source)
}
impl AdapterStore {
    fn path(&self, app: &ApplicationRecord) -> PathBuf {
        self.0.join(format!("{}.json", app.id.as_uuid()))
    }
    pub fn inspect(&self, app: &ApplicationRecord, path: &Path) -> Result<AdapterPreview, String> {
        let definition = parse(app, &read(path)?)?;
        let source = definition.to_json().map_err(|_| "invalid_adapter")?;
        let summary = summary(&definition);
        Ok(AdapterPreview { source, summary })
    }
    pub fn load(&self, app: &ApplicationRecord) -> Result<Option<Definition>, String> {
        let path = self.path(app);
        if !path.try_exists().map_err(|_| "unavailable")? {
            return Ok(None);
        }
        parse(app, &read(&path)?).map(Some)
    }
    pub fn describe(&self, app: &ApplicationRecord) -> Option<AdapterSummary> {
        match self.load(app) {
            Ok(Some(definition)) => Some(summary(&definition)),
            Ok(None) => None,
            Err(error) => Some(AdapterSummary {
                id: None,
                ready: false,
                error: Some(error),
            }),
        }
    }
    pub fn install(&self, app: &ApplicationRecord, source: &str) -> Result<(), String> {
        let definition = parse(app, source)?;
        let source = definition.to_json().map_err(|_| "invalid_adapter")?;
        std::fs::create_dir_all(&self.0).map_err(|_| "unavailable")?;
        let temp = self.0.join(format!(
            "{}.tmp",
            autologin_core::ApplicationId::new().as_uuid()
        ));
        let result = (|| {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temp).map_err(|_| "unavailable")?;
            file.write_all(source.as_bytes())
                .and_then(|_| file.sync_all())
                .map_err(|_| "unavailable")?;
            std::fs::rename(&temp, self.path(app)).map_err(|_| "unavailable")
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temp);
        }
        result.map_err(str::to_owned)
    }
    pub fn remove(&self, app: &ApplicationRecord) -> Result<(), String> {
        match std::fs::remove_file(self.path(app)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err("unavailable".into()),
        }
    }
}
fn summary(definition: &Definition) -> AdapterSummary {
    AdapterSummary {
        id: Some(definition.id.clone()),
        ready: SwitchPlan::new(definition.clone()).is_ok(),
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use autologin_core::Platform;
    fn fixture() -> (ApplicationRecord, String) {
        let app = ApplicationRecord::new(Platform::Macos, "org.example.demo".into(), "Demo".into())
            .unwrap();
        (
            app,
            include_str!("../../../adapters/examples/switcher-v2.fixture.json").into(),
        )
    }
    #[test]
    fn import_persists_across_store_instances_with_observed_login() {
        let dir = std::env::temp_dir().join(format!(
            "adapter-store-{}",
            autologin_core::ApplicationId::new().as_uuid()
        ));
        let store = AdapterStore(dir.clone());
        let (app, source) = fixture();
        assert!(store.load(&app).unwrap().is_none());
        store.install(&app, &source).unwrap();
        let restored = AdapterStore(dir.clone());
        let definition = restored.load(&app).unwrap().unwrap();
        let plan = SwitchPlan::new(definition).unwrap();
        assert_eq!(plan.logout, "logout");
        assert_eq!(plan.login, "login");
        assert!(restored.describe(&app).unwrap().ready);
        restored.remove(&app).unwrap();
        assert!(restored.load(&app).unwrap().is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn rejected_import_preserves_existing_config_and_corruption_is_visible() {
        let dir = std::env::temp_dir().join(format!(
            "adapter-store-{}",
            autologin_core::ApplicationId::new().as_uuid()
        ));
        let store = AdapterStore(dir.clone());
        let (app, source) = fixture();
        store.install(&app, &source).unwrap();
        assert_eq!(
            store.install(&app, &source.replace("org.example.demo", "other.app")),
            Err("adapter_mismatch".into())
        );
        assert!(store.install(&app, "{bad config}").is_err());
        assert!(store.load(&app).unwrap().is_some());
        std::fs::write(store.path(&app), "broken").unwrap();
        let status = store.describe(&app).unwrap();
        assert!(!status.ready);
        assert_eq!(status.error.as_deref(), Some("invalid_adapter"));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn qq_pointer_config_is_ready_for_observed_login() {
        use autologin_core::switcher::ClickMethod;
        let definition = Definition::parse(include_str!(
            "../../../adapters/local/qq-macos-v2-pointer.json"
        ))
        .unwrap();
        let plan = SwitchPlan::new(definition).unwrap();
        assert_eq!(plan.logout, "logout");
        assert_eq!(plan.login, "login");
        assert!(matches!(
            plan.definition.flows[&plan.logout].steps[0],
            Step::Click {
                method: ClickMethod::Pointer,
                ..
            }
        ));
        assert!(matches!(
            plan.definition.flows[&plan.logout].steps[1],
            Step::Click {
                method: ClickMethod::Pointer,
                ref next_states,
                ..
            } if next_states == &["quick_login".to_string(), "password".to_string()]
        ));
        assert!(matches!(
            plan.definition.flows[&plan.login].steps.as_slice(),
            [
                Step::Clear {
                    field: autologin_core::adapter::CredentialField::Username,
                    ..
                },
                Step::Clear {
                    field: autologin_core::adapter::CredentialField::Password,
                    ..
                },
                Step::Fill {
                    field: autologin_core::adapter::CredentialField::Username,
                    ..
                },
                Step::Fill {
                    field: autologin_core::adapter::CredentialField::Password,
                    ..
                },
                Step::Check { .. },
                Step::Submit { .. },
                Step::WaitState { .. }
            ]
        ));
        assert_eq!(
            plan.definition.flows[&plan.login].outcome,
            Outcome::LoggedInObserved
        );
    }
    #[test]
    fn executable_plan_rejects_ambiguous_flows_and_login_navigation() {
        let (_, source) = fixture();
        let mut definition = Definition::parse(&source).unwrap();
        definition
            .flows
            .insert("another_logout".into(), definition.flows["logout"].clone());
        assert!(SwitchPlan::new(definition).is_err());
        let mut definition = Definition::parse(&source).unwrap();
        definition.flows.get_mut("login").unwrap().steps.insert(
            0,
            Step::WaitState {
                state: "password".into(),
            },
        );
        assert!(SwitchPlan::new(definition).is_err());

        let mut with_checkbox: serde_json::Value = serde_json::from_str(&source).unwrap();
        with_checkbox["states"]["password"]["controls"]["agreement"] =
            serde_json::json!({"role":"checkbox", "names":["Accept terms"]});
        with_checkbox["flows"]["login"]["steps"]
            .as_array_mut()
            .unwrap()
            .insert(
                2,
                serde_json::json!({"type":"check", "state":"password", "target":"agreement"}),
            );
        assert!(SwitchPlan::new(Definition::parse(&with_checkbox.to_string()).unwrap()).is_ok());
        with_checkbox["states"]["password"]["controls"]["agreement"]["role"] =
            serde_json::json!("button");
        assert!(Definition::parse(&with_checkbox.to_string()).is_err());

        let definition = Definition::parse(include_str!(
            "../../../adapters/local/qq-macos-v2-pointer.json"
        ))
        .unwrap();
        for changed in [0, 1, 2] {
            let mut invalid = definition.clone();
            match changed {
                0 => invalid.flows.get_mut("login").unwrap().steps.swap(0, 1),
                1 => {
                    if let Step::Clear { state, .. } =
                        &mut invalid.flows.get_mut("login").unwrap().steps[0]
                    {
                        *state = "quick_login".into();
                    }
                }
                _ => {
                    if let Step::Clear { field, .. } =
                        &mut invalid.flows.get_mut("login").unwrap().steps[0]
                    {
                        *field = autologin_core::adapter::CredentialField::Password;
                    }
                }
            }
            assert!(SwitchPlan::new(invalid).is_err());
        }
    }
}

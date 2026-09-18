use autologin_core::{VaultService, WebsiteRecord};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read, Write},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub const EXTENSION_ID: &str = include_str!("../../../browser-extension/extension-id.txt");
pub const MAX_FRAME: usize = 32 * 1024;
pub const TTL: Duration = Duration::from_secs(60);
pub fn trusted_extension(origin: &str) -> bool {
    origin == format!("chrome-extension://{EXTENSION_ID}/")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Request {
    pub version: u8,
    pub request_id: String,
    pub body: Body,
}
#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Body {
    #[serde(rename = "account.save")]
    Save {
        origin: String,
        username: String,
        #[serde(deserialize_with = "secret")]
        password: SecretString,
    },
    #[serde(rename = "status.get")]
    Status {},
    #[serde(rename = "candidate.put")]
    Put {
        origin: String,
        username: String,
        #[serde(deserialize_with = "secret")]
        password: SecretString,
    },
    #[serde(rename = "candidate.confirm", rename_all = "camelCase")]
    Confirm {
        candidate_id: String,
        decision: Decision,
    },
    #[serde(rename = "candidate.reject", rename_all = "camelCase")]
    Reject { candidate_id: String },
}
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Save,
    Update,
}
fn secret<'de, D: serde::Deserializer<'de>>(d: D) -> Result<SecretString, D::Error> {
    String::deserialize(d).map(SecretString::from)
}
pub fn decode(bytes: &[u8]) -> Result<Request, &'static str> {
    if bytes.len() > MAX_FRAME {
        return Err("protocol.invalid");
    }
    let request: Request = serde_json::from_slice(bytes).map_err(|_| "protocol.invalid")?;
    if request.version != 1
        || request.request_id.is_empty()
        || request.request_id.len() > 64
        || !request
            .request_id
            .bytes()
            .all(|x| x.is_ascii_alphanumeric() || x == b'-')
    {
        return Err("protocol.invalid");
    }
    if let Body::Put {
        origin,
        username,
        password,
    }
    | Body::Save {
        origin,
        username,
        password,
    } = &request.body
    {
        use secrecy::ExposeSecret;
        normalized_origin(origin)?;
        if username.trim().is_empty()
            || username.len() > 512
            || password.expose_secret().is_empty()
            || password.expose_secret().len() > 4096
        {
            return Err("protocol.invalid");
        }
    }
    Ok(request)
}
pub fn normalized_origin(value: &str) -> Result<String, &'static str> {
    if value.len() > 2048 {
        return Err("protocol.invalid");
    }
    let url = url::Url::parse(value).map_err(|_| "protocol.invalid")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("protocol.invalid");
    }
    Ok(url.origin().ascii_serialization())
}
pub fn read_frame(reader: &mut impl Read) -> io::Result<Option<Zeroizing<Vec<u8>>>> {
    let mut header = [0u8; 4];
    match reader.read(&mut header[..1])? {
        0 => return Ok(None),
        1 => {}
        _ => unreachable!(),
    }
    reader.read_exact(&mut header[1..])?;
    let len = u32::from_ne_bytes(header) as usize;
    if len == 0 || len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid frame"));
    }
    let mut body = Zeroizing::new(vec![0; len]);
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}
pub fn write_frame(writer: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid response",
        ));
    }
    writer.write_all(&(bytes.len() as u32).to_ne_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub version: u8,
    pub request_id: String,
    #[serde(flatten)]
    pub body: serde_json::Value,
}
struct Candidate {
    id: String,
    origin: String,
    username: String,
    password: SecretString,
    expected: Option<WebsiteRecord>,
    expires: Instant,
    revision: i64,
}
#[derive(Default)]
pub struct Session {
    pending: Option<Candidate>,
}
impl Session {
    pub fn clear(&mut self) {
        self.pending = None;
    }
    pub fn expire(&mut self, now: Instant) {
        if self.pending.as_ref().is_some_and(|x| now >= x.expires) {
            self.clear();
        }
    }
    pub async fn handle(
        &mut self,
        request: Request,
        vault: &VaultService,
        locale: &str,
    ) -> Response {
        self.expire(Instant::now());
        let config = match vault.browser_capture_settings().await {
            Ok(value) => value,
            Err(_) => {
                self.clear();
                return Response {
                    version: 1,
                    request_id: request.request_id,
                    body: error("storage.database"),
                };
            }
        };
        if !config.enabled
            || self
                .pending
                .as_ref()
                .is_some_and(|c| c.revision != config.revision)
        {
            self.clear();
        }
        let result = match request.body {
            Body::Save { .. } | Body::Put { .. } | Body::Confirm { .. } if !config.enabled => {
                error("capture.disabled")
            }
            Body::Save {
                origin,
                username,
                password,
            } => match vault
                .capture_confirmed(&request.request_id, origin, username, password)
                .await
            {
                Ok(receipt) => {
                    serde_json::from_str(&receipt).unwrap_or_else(|_| error("internal.error"))
                }
                Err(reason) => error(reason.code()),
            },
            Body::Status {} => {
                serde_json::json!({"type":"status", "uiLocale":locale, "enabled":config.enabled, "revision":config.revision})
            }
            Body::Put {
                origin,
                username,
                password,
            } => {
                self.clear();
                match normalized_origin(&origin) {
                    Err(code) => error(code),
                    Ok(origin) => match vault.capture_target(&origin, &username).await {
                        Err(e) => error(e.code()),
                        Ok(expected) => {
                            let mode = if expected.is_some() {
                                Decision::Update
                            } else {
                                Decision::Save
                            };
                            let id = uuid::Uuid::new_v4().to_string();
                            let result = serde_json::json!({"type":"candidate.accepted", "candidateId":id, "mode":mode, "origin":origin, "username":username, "expiresIn":60});
                            self.pending = Some(Candidate {
                                id,
                                origin,
                                username,
                                password,
                                expected,
                                expires: Instant::now() + TTL,
                                revision: config.revision,
                            });
                            result
                        }
                    },
                }
            }
            Body::Reject { candidate_id } => {
                if self.pending.as_ref().is_some_and(|x| x.id == candidate_id) {
                    self.clear();
                }
                serde_json::json!({"type":"candidate.rejected"})
            }
            Body::Confirm {
                candidate_id,
                decision,
            } => match self.pending.take() {
                Some(candidate)
                    if candidate.id == candidate_id
                        && candidate.expires > Instant::now()
                        && decision
                            == if candidate.expected.is_some() {
                                Decision::Update
                            } else {
                                Decision::Save
                            } =>
                {
                    match vault
                        .save_browser_capture(
                            candidate.origin,
                            candidate.username,
                            candidate.password,
                            candidate.expected,
                            candidate.revision,
                        )
                        .await
                    {
                        Ok(_) => serde_json::json!({"type":"candidate.saved", "mode":decision}),
                        Err(e)
                            if matches!(
                                e.code(),
                                "credential.cleanup_required"
                                    | "credential.cleanup_tracking_failed"
                            ) =>
                        {
                            serde_json::json!({"type":"candidate.saved", "mode":decision, "cleanupPending":true})
                        }
                        Err(e) => error(e.code()),
                    }
                }
                _ => error("candidate.expired"),
            },
        };
        Response {
            version: 1,
            request_id: request.request_id,
            body: result,
        }
    }
}
fn error(code: &str) -> serde_json::Value {
    serde_json::json!({"type":"error", "code":code})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_and_origin_are_strict() {
        assert!(trusted_extension(&format!(
            "chrome-extension://{EXTENSION_ID}/"
        )));
        assert!(!trusted_extension(
            "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/"
        ));
        for invalid in [
            "http://example.com",
            "https://a:b@example.com",
            "https://example.com/path",
            "https://example.com/?token=x",
            "file:///tmp/x",
        ] {
            assert!(normalized_origin(invalid).is_err());
        }
        assert_eq!(
            normalized_origin("https://EXAMPLE.com:443").unwrap(),
            "https://example.com"
        );
        assert!(read_frame(&mut &(65536u32.to_ne_bytes())[..]).is_err());
        assert!(read_frame(&mut &[2, 0][..]).is_err());
        assert!(decode(br#"{"version":1,"requestId":"r","body":{"type":"shell.run"}}"#).is_err());
        assert!(decode(
            br#"{"version":1,"requestId":"r","body":{"type":"status.get","extra":true}}"#
        )
        .is_err());
        assert!(decode(br#"{"version":2,"requestId":"r","body":{"type":"status.get"}}"#).is_err());
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use autologin_core::{
        AppError, CaptureSource, CredentialStore, SaveWebsite, SecretRef, SqliteRepositories,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    #[derive(Default)]
    struct Fake {
        puts: AtomicUsize,
        deletes: AtomicUsize,
    }
    #[async_trait::async_trait]
    impl CredentialStore for Fake {
        async fn put(&self, _: &SecretRef, _: &str, _: SecretString) -> Result<(), AppError> {
            self.puts.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn reveal(&self, _: &SecretRef, _: &str) -> Result<SecretString, AppError> {
            panic!("capture must never read an old password")
        }
        async fn delete(&self, _: &SecretRef) -> Result<(), AppError> {
            self.deletes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }
    fn put() -> Request {
        decode(br#"{"version":1,"requestId":"r","body":{"type":"candidate.put","origin":"https://example.invalid","username":"alice","password":"fixture"}}"#).unwrap()
    }
    fn confirm(id: String, decision: Decision) -> Request {
        Request {
            version: 1,
            request_id: "confirm".into(),
            body: Body::Confirm {
                candidate_id: id,
                decision,
            },
        }
    }
    #[test]
    fn save_update_expiry_and_duplicate_confirmation() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let repos = SqliteRepositories::connect("sqlite::memory:")
                    .await
                    .unwrap();
                repos.migrate().await.unwrap();
                let fake = Arc::new(Fake::default());
                let vault = VaultService::new(repos.websites(), fake.clone());
                let mut session = Session::default();
                assert!(!repos.settings().browser_capture().await.unwrap().enabled);
                assert_eq!(
                    session.handle(put(), &vault, "en").await.body["code"],
                    "capture.disabled"
                );
                assert_eq!(fake.puts.load(Ordering::SeqCst), 0);
                repos.settings().set_browser_capture(true).await.unwrap();
                let accepted = session.handle(put(), &vault, "en").await;
                assert_eq!(accepted.body["mode"], "save");
                assert_eq!(fake.puts.load(Ordering::SeqCst), 0);
                let id = accepted.body["candidateId"].as_str().unwrap().to_owned();
                assert_eq!(
                    session
                        .handle(confirm(id.clone(), Decision::Save), &vault, "en")
                        .await
                        .body["type"],
                    "candidate.saved"
                );
                assert_eq!(
                    session
                        .handle(confirm(id, Decision::Save), &vault, "en")
                        .await
                        .body["type"],
                    "error"
                );
                assert_eq!(fake.puts.load(Ordering::SeqCst), 1);
                assert_eq!(
                    vault.list_websites().await.unwrap()[0].capture_source,
                    CaptureSource::BrowserExtension
                );
                let accepted = session.handle(put(), &vault, "en").await;
                assert_eq!(accepted.body["mode"], "update");
                let id = accepted.body["candidateId"].as_str().unwrap().to_owned();
                assert_eq!(
                    session
                        .handle(confirm(id, Decision::Update), &vault, "en")
                        .await
                        .body["type"],
                    "candidate.saved"
                );
                assert_eq!(fake.puts.load(Ordering::SeqCst), 2);
                assert_eq!(fake.deletes.load(Ordering::SeqCst), 1);
                let accepted = session.handle(put(), &vault, "en").await;
                session.expire(Instant::now() + TTL);
                assert_eq!(
                    session
                        .handle(
                            confirm(
                                accepted.body["candidateId"].as_str().unwrap().into(),
                                Decision::Update
                            ),
                            &vault,
                            "en"
                        )
                        .await
                        .body["type"],
                    "error"
                );
                assert_eq!(fake.puts.load(Ordering::SeqCst), 2);
                let accepted = session.handle(put(), &vault, "en").await;
                let existing = vault.list_websites().await.unwrap().remove(0);
                vault
                    .save_website(SaveWebsite::new(
                        Some(existing.id),
                        "Changed elsewhere",
                        existing.url,
                        existing.username,
                        None::<SecretString>,
                        "notes",
                    ))
                    .await
                    .unwrap();
                let result = session
                    .handle(
                        confirm(
                            accepted.body["candidateId"].as_str().unwrap().into(),
                            Decision::Update,
                        ),
                        &vault,
                        "en",
                    )
                    .await;
                assert_eq!(result.body["code"], "storage.conflict");
                assert_eq!(fake.puts.load(Ordering::SeqCst), 2);
                session.handle(put(), &vault, "en").await;
                session.clear();
                assert!(session.pending.is_none());
                let accepted = session.handle(put(), &vault, "en").await;
                repos.settings().set_browser_capture(false).await.unwrap();
                assert_eq!(
                    session
                        .handle(
                            confirm(
                                accepted.body["candidateId"].as_str().unwrap().into(),
                                Decision::Update
                            ),
                            &vault,
                            "en"
                        )
                        .await
                        .body["code"],
                    "capture.disabled"
                );
                assert_eq!(fake.puts.load(Ordering::SeqCst), 2);
                repos.settings().set_browser_capture(true).await.unwrap();
                let accepted = session.handle(put(), &vault, "en").await;
                repos.settings().set_browser_capture(false).await.unwrap();
                repos.settings().set_browser_capture(true).await.unwrap();
                assert_eq!(
                    session
                        .handle(
                            confirm(
                                accepted.body["candidateId"].as_str().unwrap().into(),
                                Decision::Update
                            ),
                            &vault,
                            "en"
                        )
                        .await
                        .body["code"],
                    "candidate.expired"
                );
                assert_eq!(fake.puts.load(Ordering::SeqCst), 2);
            });
    }
    #[test]
    fn independent_connections_cannot_both_create_same_confirmed_origin() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let directory = std::env::temp_dir()
                    .join(format!("logindeck-capture-{}", uuid::Uuid::new_v4()));
                std::fs::create_dir(&directory).unwrap();
                let url = format!("sqlite://{}", directory.join("vault.sqlite3").display());
                let a = SqliteRepositories::connect(&url).await.unwrap();
                a.migrate().await.unwrap();
                a.settings().set_browser_capture(true).await.unwrap();
                let b = SqliteRepositories::connect(&url).await.unwrap();
                let fake = Arc::new(Fake::default());
                let va = VaultService::new(a.websites(), fake.clone());
                let vb = VaultService::new(b.websites(), fake.clone());
                let first = tokio::spawn(async move {
                    va.save_browser_capture(
                        "https://example.invalid".into(),
                        "alice".into(),
                        SecretString::from("fixture"),
                        None,
                        1,
                    )
                    .await
                });
                let second = tokio::spawn(async move {
                    vb.save_browser_capture(
                        "https://example.invalid".into(),
                        "alice".into(),
                        SecretString::from("fixture"),
                        None,
                        1,
                    )
                    .await
                });
                let first = first.await.unwrap();
                let second = second.await.unwrap();
                assert_ne!(first.is_ok(), second.is_ok());
                assert_eq!(fake.puts.load(Ordering::SeqCst), 1);
                drop(a);
                drop(b);
                std::fs::remove_dir_all(directory).unwrap();
            });
    }
}

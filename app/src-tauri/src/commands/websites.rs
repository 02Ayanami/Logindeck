use crate::{commands::CommandError, state::AppState};
use autologin_core::{SaveWebsite, WebsiteId, WebsiteRecord};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Deserialize)]
pub struct SaveWebsiteRequest {
    #[serde(default)]
    pub account_name: Option<String>,
    pub id: Option<WebsiteId>,
    pub name: String,
    pub url: String,
    pub username: String,
    pub password: Option<String>,
    #[serde(default)]
    pub notes: String,
}
impl std::fmt::Debug for SaveWebsiteRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SaveWebsiteRequest")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("url", &self.url)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("notes", &self.notes)
            .finish()
    }
}
#[derive(Clone, Serialize)]
pub struct WebsiteResponse {
    pub account_name: String,
    pub id: WebsiteId,
    pub name: String,
    pub url: String,
    pub normalized_origin: String,
    pub username: String,
    pub notes: String,
    pub capture_source: autologin_core::CaptureSource,
    pub created_at: String,
    pub updated_at: String,
}
impl From<WebsiteRecord> for WebsiteResponse {
    fn from(x: WebsiteRecord) -> Self {
        Self {
            account_name: x.account_name,
            id: x.id,
            name: x.name,
            url: x.url,
            normalized_origin: x.normalized_origin,
            username: x.username,
            notes: x.notes,
            capture_source: x.capture_source,
            created_at: x.created_at,
            updated_at: x.updated_at,
        }
    }
}

#[tauri::command]
pub async fn next_account_number(
    scope: String,
    state: State<'_, AppState>,
) -> Result<i64, CommandError> {
    state
        .vault
        .next_account_number(&scope)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn edit_website_group(
    origin: String,
    name: String,
    url: String,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    state
        .vault
        .edit_website_group(origin, name, url)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn list_websites(
    state: State<'_, AppState>,
) -> Result<Vec<WebsiteResponse>, CommandError> {
    Ok(state
        .vault
        .list_websites()
        .await?
        .into_iter()
        .map(Into::into)
        .collect())
}
#[tauri::command]
pub async fn save_website(
    request: SaveWebsiteRequest,
    state: State<'_, AppState>,
) -> Result<WebsiteResponse, CommandError> {
    let mut input = SaveWebsite::new(
        request.id,
        request.name,
        request.url,
        request.username,
        request.password.map(SecretString::from),
        request.notes,
    );
    input.account_name = request.account_name;
    state
        .vault
        .save_website(input)
        .await
        .map(Into::into)
        .map_err(Into::into)
}
#[tauri::command]
pub async fn delete_website(id: WebsiteId, state: State<'_, AppState>) -> Result<(), CommandError> {
    state.vault.delete_website(id).await.map_err(Into::into)
}
#[tauri::command]
pub async fn copy_website_username(
    id: WebsiteId,
    state: State<'_, AppState>,
) -> Result<String, CommandError> {
    state
        .vault
        .copy_website_username(id)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn copy_website_password(
    id: WebsiteId,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    state
        .vault
        .copy_website_password(id, state.presence.clone(), state.clipboard.clone())
        .await
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::WebsiteResponse;
    use autologin_core::{CaptureSource, SecretRef, WebsiteId, WebsiteRecord};

    #[test]
    fn website_response_serialization_excludes_the_credential_reference() {
        let response = WebsiteResponse::from(WebsiteRecord {
            account_name: "Account 1".into(),
            id: WebsiteId::new(),
            name: "Example".into(),
            url: "https://example.com".into(),
            normalized_origin: "https://example.com".into(),
            username: "alice".into(),
            password_credential_ref: SecretRef::new("opaque-keychain-reference").unwrap(),
            capture_source: CaptureSource::Manual,
            notes: String::new(),
            created_at: "1".into(),
            updated_at: "1".into(),
        });
        let value = serde_json::to_value(response).unwrap();
        assert!(value.get("password_credential_ref").is_none());
        assert!(!value.to_string().contains("opaque-keychain-reference"));
    }
}

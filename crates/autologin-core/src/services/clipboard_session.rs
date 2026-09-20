use std::sync::Arc;

use secrecy::SecretString;
use tokio::sync::Mutex;

use crate::{AppError, Clipboard};

struct Session<T> {
    token: Option<Arc<T>>,
    closed: bool,
}

/// Tracks only the last write token, never clipboard contents. Shutdown and writes share a lane
/// so an authentication operation that finishes during exit cannot leave a new password behind.
pub struct ClipboardSession<B: Clipboard> {
    backend: B,
    state: Mutex<Session<B::ChangeToken>>,
}

impl<B: Clipboard> ClipboardSession<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            state: Mutex::new(Session {
                token: None,
                closed: false,
            }),
        }
    }

    pub async fn shutdown(&self) -> Result<(), AppError> {
        let mut state = self.state.lock().await;
        state.closed = true;
        if let Some(token) = &state.token {
            self.backend.clear_if_unchanged(token).await?;
        }
        state.token = None;
        Ok(())
    }
}

#[async_trait::async_trait]
impl<B: Clipboard> Clipboard for ClipboardSession<B> {
    type ChangeToken = Arc<B::ChangeToken>;

    async fn write(&self, value: SecretString) -> Result<Self::ChangeToken, AppError> {
        let mut state = self.state.lock().await;
        if state.closed {
            return Err(AppError::new("clipboard.unavailable"));
        }
        let token = Arc::new(self.backend.write(value).await?);
        state.token = Some(token.clone());
        Ok(token)
    }

    async fn clear_if_unchanged(&self, token: &Self::ChangeToken) -> Result<(), AppError> {
        let mut state = self.state.lock().await;
        if state
            .token
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, token))
        {
            self.backend.clear_if_unchanged(token).await?;
            state.token = None;
        }
        Ok(())
    }
}

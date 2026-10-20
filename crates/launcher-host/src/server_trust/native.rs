//! A join-local native question and the existing public-key trust file.

mod store;
#[cfg(test)]
mod tests;

use super::{ServerTrustPrompt, TrustSource};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures::future::BoxFuture;
use p384::{ecdsa::VerifyingKey, pkcs8::EncodePublicKey};
use std::{
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};
use tokio::sync::oneshot;

struct Question {
    id: u64,
    endpoint: String,
    answer: oneshot::Sender<bool>,
}

struct Shared {
    alive: AtomicBool,
    question: Mutex<Option<Question>>,
}

/// Owns one menu-visible native join's trust lifetime, independently of account/Core polling.
pub struct NativeSessionTrust {
    shared: Arc<Shared>,
    path: PathBuf,
}

struct Authorization {
    shared: Arc<Shared>,
    path: PathBuf,
}

impl NativeSessionTrust {
    /// Selects the existing installed public-key file without reading it on the frame thread.
    pub fn new(path: PathBuf) -> Self {
        Self {
            shared: Arc::new(Shared {
                alive: AtomicBool::new(true),
                question: Mutex::new(None),
            }),
            path,
        }
    }

    /// Gives the original network future only a decision capability for this exact UI lifetime.
    pub fn authorization(&self) -> Arc<dyn protocol::NativeServerTrust> {
        Arc::new(Authorization {
            shared: Arc::clone(&self.shared),
            path: self.path.clone(),
        })
    }
}

impl Drop for NativeSessionTrust {
    fn drop(&mut self) {
        self.shared.alive.store(false, Ordering::Release);
        self.shared
            .question
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
    }
}

impl TrustSource for NativeSessionTrust {
    fn prompt(&self) -> Option<ServerTrustPrompt> {
        self.shared
            .question
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .map(|question| ServerTrustPrompt {
                id: question.id,
                url: question.endpoint.clone(),
                // This flag routes to the original session TrustSource, including native joins.
                from_session_core: true,
            })
    }

    fn answer(&self, id: u64, trusted: bool) {
        let mut question = self
            .shared
            .question
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if question.as_ref().is_some_and(|question| question.id == id)
            && let Some(question) = question.take()
        {
            let _ = question.answer.send(trusted);
        }
    }
}

impl protocol::NativeServerTrust for Authorization {
    fn trust<'a>(
        &'a self,
        endpoint: &'a str,
        public_key: &'a VerifyingKey,
        deadline: Instant,
    ) -> BoxFuture<'a, io::Result<bool>> {
        Box::pin(async move {
            self.current(deadline)?;
            let der = public_key.to_public_key_der().map_err(|_| unavailable())?;
            let key = STANDARD.encode(der.as_bytes());
            let decision = async {
                if store::known(&self.path, &key).await {
                    self.current(deadline)?;
                    return Ok(true);
                }
                if !endpoint.starts_with("https://") && !self.confirm(endpoint, deadline).await? {
                    return Ok(false);
                }
                self.current(deadline)?;
                store::remember(&self.path, key).await;
                self.current(deadline)?;
                Ok(true)
            };
            tokio::time::timeout_at(deadline.into(), decision)
                .await
                .map_err(|_| expired())?
        })
    }
}

impl Authorization {
    /// Publishes one bounded question; cancellation withdraws only this exact pending identity.
    async fn confirm(&self, endpoint: &str, deadline: Instant) -> io::Result<bool> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT
            .try_update(Ordering::AcqRel, Ordering::Acquire, |id| id.checked_add(1))
            .map_err(|_| unavailable())?;
        let (answer, receive) = oneshot::channel();
        {
            let mut question = self
                .shared
                .question
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            self.current(deadline)?;
            if question.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "native trust question is already pending",
                ));
            }
            *question = Some(Question {
                id,
                endpoint: endpoint.to_owned(),
                answer,
            });
        }
        let _pending = Pending {
            shared: &self.shared,
            id,
        };
        let answer = receive.await.map_err(|_| unavailable())?;
        self.current(deadline)?;
        Ok(answer)
    }

    /// Rejects a retired UI owner and the original join cutoff without touching account credentials.
    fn current(&self, deadline: Instant) -> io::Result<()> {
        if !self.shared.alive.load(Ordering::Acquire) {
            return Err(unavailable());
        }
        if Instant::now() >= deadline {
            return Err(expired());
        }
        Ok(())
    }
}

struct Pending<'a> {
    shared: &'a Shared,
    id: u64,
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        let mut question = self
            .shared
            .question
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if question
            .as_ref()
            .is_some_and(|question| question.id == self.id)
        {
            question.take();
        }
    }
}

/// Keeps key and selected endpoint values out of diagnostics.
fn unavailable() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotConnected,
        "native server trust is unavailable",
    )
}
/// Preserves the original whole-join cutoff while a question is visible.
fn expired() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "native server trust timed out")
}

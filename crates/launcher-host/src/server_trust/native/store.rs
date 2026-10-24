//! Compatible oldest-first public-key persistence; no account or token is stored.

use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read, Write},
    path::Path,
    sync::{Mutex, OnceLock},
};

pub(super) const MAX_TRUSTED_KEYS: usize = 100;
const MAX_STORE_BYTES: usize = 64 * 1024;

#[derive(Default, Deserialize, Serialize)]
struct Document {
    #[serde(default, deserialize_with = "keys")]
    keys: Vec<String>,
}

/// Preserves JSON null's zero string/slice values in the existing Go-compatible trust document.
fn keys<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    Option::<Vec<Option<String>>>::deserialize(deserializer).map(|keys| {
        keys.unwrap_or_default()
            .into_iter()
            .map(Option::unwrap_or_default)
            .collect()
    })
}

/// Serializes native read/modify/replace operations without holding a lock during user decisions.
fn mutations() -> &'static Mutex<()> {
    static MUTATIONS: OnceLock<Mutex<()>> = OnceLock::new();
    MUTATIONS.get_or_init(Mutex::default)
}

/// Moves a known key to most-recent use; persistence failure never revokes the known trust.
pub(super) async fn known(path: &Path, key: &str) -> bool {
    let (path, key) = (path.to_owned(), key.to_owned());
    tokio::task::spawn_blocking(move || {
        let _mutation = mutations()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(mut document) = load(&path) else {
            return false;
        };
        let Some(index) = document.keys.iter().position(|known| known == &key) else {
            return false;
        };
        let key = document.keys.remove(index);
        document.keys.push(key);
        save_or_warn(&path, &document);
        true
    })
    .await
    .unwrap_or(false)
}

/// Re-reads after approval so an unreadable original file is never replaced with a partial history.
/// Once admitted, this bounded transaction may finish after the approved join stops waiting.
pub(super) async fn remember(path: &Path, key: String) {
    let path = path.to_owned();
    let result = tokio::task::spawn_blocking(move || {
        let _mutation = mutations()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(mut document) = load(&path) else {
            return;
        };
        if document.keys.contains(&key) {
            return;
        }
        document.keys.push(key);
        let excess = document.keys.len().saturating_sub(MAX_TRUSTED_KEYS);
        document.keys.drain(..excess);
        save_or_warn(&path, &document);
    })
    .await;
    if result.is_err() {
        tracing::warn!("native server trust persistence did not complete");
    }
}

/// Treats a missing file as empty; malformed, oversized or unreadable stores retain their bytes.
fn load(path: &Path) -> Option<Document> {
    let result = (|| {
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Document::default()),
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        let limit = MAX_STORE_BYTES;
        file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(io::Error::other("server trust file exceeds limit"));
        }
        if bytes.trim_ascii() != b"null" && !bytes.trim_ascii_start().starts_with(b"{") {
            return Err(io::Error::other(
                "server trust document must be an object or null",
            ));
        }
        // Serde remains the sole grammar owner, including complete null and trailing-data checks.
        serde_json::from_slice::<Option<Document>>(&bytes)
            .map(Option::unwrap_or_default)
            .map_err(io::Error::other)
    })();
    match result {
        Ok(document) => Some(document),
        Err(_) => {
            tracing::warn!("could not read native server trust file");
            None
        }
    }
}

/// Reports a failed replacement without changing the already completed authorization decision.
fn save_or_warn(path: &Path, document: &Document) {
    if save(path, document).is_err() {
        tracing::warn!("could not save native server trust file");
    }
}

/// Owns the complete atomic replacement and temp guard even if its approved join stops waiting.
fn save(path: &Path, document: &Document) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("server trust directory is unavailable"))?;
    let mut directory = std::fs::DirBuilder::new();
    directory.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        directory.mode(0o700);
    }
    directory.create(parent)?;
    let mut file = tempfile::Builder::new()
        .prefix(".server-trust-")
        .suffix(".tmp")
        .tempfile_in(parent)?;
    let bytes = serde_json::to_vec(document).map_err(io::Error::other)?;
    file.write_all(&bytes)?;
    file.flush()?;
    file.persist(path).map(drop).map_err(|error| error.error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        task::{Context, Waker},
        time::Duration,
    };

    /// Positional arrays cannot supply trust or replace an unreadable original document.
    #[tokio::test]
    async fn array_root_trust_files_never_grant_known_keys_or_replace_original_bytes() {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        use p384::{ecdsa::SigningKey, elliptic_curve::Generate, pkcs8::EncodePublicKey};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("trusted.json");
        let key = SigningKey::generate();
        let key = STANDARD.encode(key.verifying_key().to_public_key_der().unwrap().as_bytes());
        let originals = [
            b"[]".to_vec(),
            serde_json::to_vec(&serde_json::json!([[key]])).unwrap(),
        ];
        let mut observations = Vec::new();
        for original in originals {
            tokio::fs::write(&path, &original).await.unwrap();
            let known = known(&path, &key).await;
            let after_read = tokio::fs::read(&path).await.unwrap();
            remember(&path, "independently-approved-new-key".into()).await;
            let after_write = tokio::fs::read(&path).await.unwrap();
            observations.push((original, known, after_read, after_write));
        }
        // Every actual blocking transaction has completed before either behavioral assertion.
        assert!(
            observations
                .iter()
                .all(|(original, _, read, written)| original == read && original == written),
            "an array-root trust document replaced unreadable original history"
        );
        assert!(
            observations.iter().all(|(_, known, _, _)| !known),
            "an array-root trust document granted silent known-key authority"
        );
    }

    /// Original object/null and missing/null-key defaults remain ordinary readable empty history.
    #[tokio::test]
    async fn object_and_null_trust_files_keep_the_original_empty_history_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("trusted.json");
        for original in ["null", "{}", "{\"keys\":null}"] {
            tokio::fs::write(&path, original).await.unwrap();
            remember(&path, "independently-approved-key".into()).await;
            assert!(known(&path, "independently-approved-key").await);
            let actual: serde_json::Value =
                serde_json::from_slice(&tokio::fs::read(&path).await.unwrap()).unwrap();
            assert_eq!(
                actual,
                serde_json::json!({"keys":["independently-approved-key"]})
            );
        }
    }

    /// Canceling the waiter retains the original admitted transaction and its exact temp guard.
    #[tokio::test]
    async fn approved_transaction_finishes_atomically_after_its_waiter_is_canceled() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("trusted.json");
        {
            let _blocked = mutations().lock().unwrap();
            let mut write = Box::pin(remember(&path, "already-approved-key".into()));
            let mut context = Context::from_waker(Waker::noop());
            assert!(write.as_mut().poll(&mut context).is_pending());
            drop(write);
        }
        test_time::eventually_within(
            Duration::from_secs(5),
            "the original approved trust transaction",
            || path.exists(),
        );
        let _settled = mutations().lock().unwrap();
        let document = load(&path).unwrap();
        assert_eq!(document.keys, ["already-approved-key"]);
        assert_eq!(
            std::fs::read_dir(directory.path()).unwrap().count(),
            1,
            "the same atomic transaction must retire its temp file"
        );
    }
}

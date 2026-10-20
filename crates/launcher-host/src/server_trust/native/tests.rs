use super::*;
use p384::{ecdsa::SigningKey, elliptic_curve::Generate};
use std::time::Duration;

/// Waits on the actual authorization future and filesystem progress until its original question exists.
async fn question(
    owner: &NativeSessionTrust,
    decision: &mut std::pin::Pin<
        Box<dyn std::future::Future<Output = io::Result<bool>> + Send + '_>,
    >,
) -> ServerTrustPrompt {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            tokio::select! { biased;
                result = decision.as_mut() => panic!("trust completed before its question: {result:?}"),
                _ = tokio::task::yield_now() => {},
            }
            if let Some(question) = owner.prompt() { return question; }
        }
    }).await.expect("actual native trust question stalled")
}

/// The existing file grants a key across addresses only after one explicit native HTTP approval.
#[tokio::test]
async fn native_http_approval_persists_the_actual_key_and_later_use_is_silent() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("trusted.json");
    let owner = NativeSessionTrust::new(path.clone());
    let authorization = owner.authorization();
    let key = SigningKey::generate();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut decision = authorization.trust(
        "http://fixture.invalid:19132",
        key.verifying_key(),
        deadline,
    );
    let prompt = question(&owner, &mut decision).await;
    assert!(
        prompt.from_session_core,
        "the original session dialog must own this answer"
    );
    owner.answer(prompt.id, true);
    assert!(decision.await.unwrap());
    assert!(owner.prompt().is_none());
    let bytes = tokio::fs::read(&path).await.unwrap();
    let stored: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let expected = STANDARD.encode(key.verifying_key().to_public_key_der().unwrap().as_bytes());
    assert_eq!(stored["keys"], serde_json::json!([expected]));
    assert!(
        authorization
            .trust("http://other.invalid:19132", key.verifying_key(), deadline)
            .await
            .unwrap()
    );
    assert!(owner.prompt().is_none());
}

/// Canceling a borrowed decision withdraws only that question; stale answers cannot approve a successor.
#[tokio::test]
async fn canceled_native_question_and_retired_ui_do_not_authorize_later_joins() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("trusted.json");
    let owner = NativeSessionTrust::new(path.clone());
    let authorization = owner.authorization();
    let key = SigningKey::generate();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut first = authorization.trust(
        "http://fixture.invalid:19132",
        key.verifying_key(),
        deadline,
    );
    let old = question(&owner, &mut first).await;
    drop(first);
    assert!(owner.prompt().is_none());
    let mut second = authorization.trust(
        "http://fixture.invalid:19132",
        key.verifying_key(),
        deadline,
    );
    let current = question(&owner, &mut second).await;
    assert_ne!(old.id, current.id);
    owner.answer(old.id, true);
    assert_eq!(owner.prompt(), Some(current.clone()));
    owner.answer(current.id, false);
    assert!(!second.await.unwrap());
    assert!(
        !path.exists(),
        "declined or canceled decisions must not persist trust"
    );
    let mut third = authorization.trust(
        "http://fixture.invalid:19132",
        key.verifying_key(),
        deadline,
    );
    question(&owner, &mut third).await;
    drop(owner);
    assert_eq!(third.await.unwrap_err().kind(), io::ErrorKind::NotConnected);
    assert!(!path.exists());
}

/// HTTPS approval is silent, while a malformed original store is preserved after explicit HTTP approval.
#[tokio::test]
async fn native_trust_keeps_https_and_unreadable_store_policy_separate() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("trusted.json");
    let owner = NativeSessionTrust::new(path.clone());
    let authorization = owner.authorization();
    let key = SigningKey::generate();
    let deadline = Instant::now() + Duration::from_secs(10);
    assert!(
        authorization
            .trust("https://fixture.invalid:443", key.verifying_key(), deadline)
            .await
            .unwrap()
    );
    assert!(owner.prompt().is_none());
    let invalid = b"{unreadable original trust document";
    tokio::fs::write(&path, invalid).await.unwrap();
    let other = SigningKey::generate();
    let mut decision = authorization.trust(
        "http://fixture.invalid:19132",
        other.verifying_key(),
        deadline,
    );
    let prompt = question(&owner, &mut decision).await;
    owner.answer(prompt.id, true);
    assert!(decision.await.unwrap());
    assert_eq!(tokio::fs::read(&path).await.unwrap(), invalid);
}

/// Actual file mutations preserve the oldest-first capacity and make a known use most recent.
#[tokio::test]
async fn native_trusted_key_lru_retains_only_the_latest_original_keys() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("trusted.json");
    let original: Vec<_> = (0..store::MAX_TRUSTED_KEYS)
        .map(|value| value.to_string())
        .collect();
    tokio::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({"keys":original})).unwrap(),
    )
    .await
    .unwrap();
    store::remember(&path, "new-key".into()).await;
    let mut stored: serde_json::Value =
        serde_json::from_slice(&tokio::fs::read(&path).await.unwrap()).unwrap();
    let keys = stored["keys"].as_array_mut().unwrap();
    assert_eq!(keys.len(), store::MAX_TRUSTED_KEYS);
    assert_eq!(keys[0], "1");
    assert!(store::known(&path, "1").await);
    let stored: serde_json::Value =
        serde_json::from_slice(&tokio::fs::read(&path).await.unwrap()).unwrap();
    assert_eq!(stored["keys"].as_array().unwrap().last().unwrap(), "1");
}

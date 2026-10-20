use super::*;
use serde::Deserialize;
use std::{
    future::Future,
    task::{Context, Poll, Waker},
};

/// The completed invite list remains owned while its original verified account is checked out.
#[tokio::test(start_paused = true)]
async fn completed_native_read_survives_same_account_checkout() {
    let (_directory, account) = super::super::profile::tests::account();
    let loaded = {
        let state = account.state.lock().unwrap();
        let cached = state.cached.as_ref().unwrap();
        NativePeople {
            people: vec![bridge::Person {
                xuid: "456".into(),
                gamertag: "Completed friend".into(),
                ..Default::default()
            }],
            identity: ReadIdentity {
                account: Arc::clone(&account),
                generation: cached.generation.clone(),
                xuid: cached.identity.as_ref().unwrap().xuid.clone(),
            },
        }
    };
    let contender = account.begin().unwrap();
    let checked_out = {
        let state = account.state.lock().unwrap();
        state.busy && state.cached.is_none()
    };
    let mut publishing = Box::pin(async { loaded.into_current() });
    let first = publishing
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()));
    let retained = first.is_pending();
    drop(contender);
    let people = match first {
        Poll::Pending => publishing.await,
        Poll::Ready(people) => people,
    };
    assert!(checked_out, "the actual account owner was not checked out");
    assert!(
        retained,
        "ordinary account checkout discarded the completed invite people"
    );
    let people = people.expect("the completed original people remain current");
    assert_eq!(people[0].gamertag, "Completed friend");
}

/// Both actual Go and SDK-decoded native projections use the same response/expectation corpus.
#[test]
fn native_invite_people_match_shared_go_projection_vectors() {
    #[derive(Deserialize)]
    struct Case {
        name: String,
        people: Vec<User>,
        expected: Vec<bridge::Person>,
    }
    let corpus = include_str!("../../../../../core/catalog/native_people_conformance.json")
        .replace("$SIDE", &bridge::contract::INVITE_GAMERPIC_SIDE.to_string());
    let cases: Vec<Case> = serde_json::from_str(&corpus).unwrap();
    for case in cases {
        assert_eq!(project(case.people), case.expected, "{}", case.name);
    }
    let users = (0..=bridge::contract::INVITE_MAX_PEOPLE).map(|index| {
        serde_json::from_value(serde_json::json!({
            "xuid":index.to_string(), "gamertag":format!("offline{index:04}"), "presenceState":"Offline"
        })).unwrap()
    }).chain(std::iter::once(serde_json::from_value(serde_json::json!({
        "xuid":"online", "gamertag":"zz-online", "presenceState":"Online"
    })).unwrap())).collect();
    let people = project(users);
    assert_eq!(people.len(), bridge::contract::INVITE_MAX_PEOPLE);
    assert!(people[0].online);
    assert_eq!(people[0].gamertag, "zz-online");
    assert!(picture("https://127.0.0.1/private").is_none());
}

/// The real protected lease admits ordinary refresh but defeats replacement identity after artwork.
#[test]
fn native_invite_people_recheck_account_after_artwork() {
    let (directory, account) = super::super::profile::tests::account();
    let loaded = || NativePeople {
        people: vec![bridge::Person {
            xuid: "456".into(),
            gamertag: "Friend".into(),
            ..Default::default()
        }],
        identity: ReadIdentity {
            account: Arc::clone(&account),
            generation: "original".into(),
            xuid: "123".into(),
        },
    };
    let mut lease = account.store.try_native_lease().unwrap();
    lease.suppress_checkpoint().unwrap();
    {
        let mut state = account.state.lock().unwrap();
        let cached = state.cached.as_mut().unwrap();
        cached.binding = lease.binding();
        cached.checkpoint_revision = lease.checkpoint_revision();
    }
    drop(lease);
    assert_eq!(loaded().into_current().unwrap()[0].gamertag, "Friend");
    account
        .state
        .lock()
        .unwrap()
        .cached
        .as_mut()
        .unwrap()
        .identity
        .as_mut()
        .unwrap()
        .xuid = "789".into();
    assert!(loaded().into_current().is_none());
    account
        .state
        .lock()
        .unwrap()
        .cached
        .as_mut()
        .unwrap()
        .identity
        .as_mut()
        .unwrap()
        .xuid = "123".into();
    super::super::profile::tests::seed(&directory.path().join("oauth.json"), "replacement");
    assert!(loaded().into_current().is_none());
}

/// Canceling the real acquisition waiter cannot release the live session's original lease owner.
#[tokio::test]
async fn cancelled_native_people_read_keeps_existing_account_operation() {
    let (_directory, account) = super::super::profile::tests::account();
    let operation = account.begin().unwrap();
    let mut reading = Box::pin(Arc::clone(&account).read_people());
    assert!(
        reading
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    drop(reading);
    assert!(account.state.lock().unwrap().busy);
    drop(operation);
    assert!(!account.state.lock().unwrap().busy);
    assert!(account.state.lock().unwrap().cached.is_some());
}

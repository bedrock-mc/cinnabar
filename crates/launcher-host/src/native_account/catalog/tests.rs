use super::*;
use std::{
    future::Future,
    task::{Context, Poll, Waker},
};

/// A completed section stays owned while another real operation has checked out its account.
#[tokio::test(start_paused = true)]
async fn completed_native_read_survives_same_account_checkout() {
    let (_directory, account) = super::super::profile::tests::account();
    let listing = {
        let state = account.state.lock().unwrap();
        let cached = state.cached.as_ref().unwrap();
        CatalogListing {
            cards: CatalogCards {
                realms: Ok(vec![
                    serde_json::from_value(serde_json::json!({
                        "name": "Completed catalog", "state": "OPEN"
                    }))
                    .unwrap(),
                ]),
                friends: Err(io::ErrorKind::TimedOut.into()),
            },
            generation: cached.generation.clone(),
            binding: cached.binding,
        }
    };
    let contender = account.begin().unwrap();
    let checked_out = {
        let state = account.state.lock().unwrap();
        state.busy && state.cached.is_none()
    };
    // The async block uses the original finalizer; no service or replacement read is started.
    let mut publishing = Box::pin(async { listing.into_current_cards(&account) });
    let first = publishing
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()));
    let retained = first.is_pending();
    drop(contender);
    let cards = match first {
        Poll::Pending => publishing.await,
        Poll::Ready(cards) => cards,
    };
    assert!(checked_out, "the actual account owner was not checked out");
    assert!(
        retained,
        "ordinary account checkout discarded the completed catalog"
    );
    let cards = cards.expect("the completed original catalog remains current");
    assert_eq!(cards.realms.unwrap()[0].name, "Completed catalog");
    assert!(matches!(cards.friends, Err(error) if error.kind() == io::ErrorKind::TimedOut));
}

//! Saved account operations run after the account core releases its cache.
use crossbeam_channel::{Receiver, bounded};
use launcher::{
    accounts::{AccountProfile, AccountStore},
    menu::view::MenuProfile,
};

#[derive(Debug)]
pub enum Operation {
    Switch(String),
    Commit(AccountProfile),
    CommitNative(AccountProfile, launcher::accounts::pending::PendingSignIn),
    RestoreNative(String),
    Restore,
    SignOut,
}

/// Queues a profile save and returns its completion only when the worker starts.
pub fn remember(store: AccountStore, profile: MenuProfile) -> Option<Receiver<bool>> {
    let (sender, receiver) = bounded(1);
    std::thread::Builder::new()
        .name("account-save".into())
        .spawn(move || {
            let success = store
                .remember_current(
                    &profile.xuid,
                    &profile.gamertag,
                    (!profile.picture_path.is_empty()).then_some(profile.picture_path.as_str()),
                )
                .is_ok();
            let _ = sender.send(success);
        })
        .ok()?;
    Some(receiver)
}

/// Prepares a cache mutation to run once the caller has stopped its owning core.
pub fn operation_job(
    store: AccountStore,
    operation: Operation,
) -> (impl FnOnce() + Send + 'static, Receiver<(bool, bool)>) {
    let (sender, receiver) = bounded(1);
    let job = move || {
        let signed_out = matches!(&operation, Operation::SignOut);
        let native = matches!(
            &operation,
            Operation::CommitNative(..) | Operation::RestoreNative(..)
        );
        let result = match operation {
            Operation::Switch(id) => store.activate(&id),
            Operation::Commit(profile) => store
                .commit_pending(
                    &profile.id,
                    &profile.gamertag,
                    profile.picture_path.as_deref(),
                )
                .map(|_| ()),
            Operation::CommitNative(profile, pending) => pending.try_commit(&profile),
            Operation::RestoreNative(generation) => store.try_discard_native_pending(&generation),
            Operation::Restore => Ok(()),
            Operation::SignOut => store.sign_out(),
        };
        if !native {
            let _ = store.discard_pending();
        }
        let _ = sender.send((result.is_ok(), signed_out));
    };
    (job, receiver)
}

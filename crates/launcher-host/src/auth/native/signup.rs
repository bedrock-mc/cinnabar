//! One hosted profile prompt and finite retries on the original saved or pending account.

use super::*;
use tokio::time::Instant;

/// Keeps the first hosted signup window across borrowed acquisition cancellation and repeated challenges.
#[derive(Default)]
pub(super) struct Signup {
    deadline: Option<Instant>,
}

impl Signup {
    /// Presents only the first validated signed page and captures its one absolute retry cutoff.
    pub(super) fn present(
        &mut self,
        uri: &str,
        now: Instant,
        sender: &Sender<Message>,
    ) -> Result<(), Failure> {
        if self.deadline.is_none() {
            if !super::super::valid_https_uri(uri) || uri.len() > super::super::MAX_LINE_BYTES {
                return Err(Failure::Failed(
                    "Could not open Xbox profile setup. Try signing in again.",
                ));
            }
            send(
                sender,
                Message::State(AuthState::AwaitingXboxSignup { uri: uri.into() }),
            )?;
            self.deadline = Some(
                now + Duration::from_secs(u64::from(bridge::contract::AUTH_XBOX_SIGNUP_SECONDS)),
            );
        }
        self.check(now)
    }

    /// Caps each actual acquisition by its original service budget and the first signup window.
    pub(super) fn acquisition_cutoff(&self, now: Instant) -> Result<Instant, Failure> {
        self.check(now)?;
        let cutoff = now + ACQUISITION_TIMEOUT;
        Ok(self
            .deadline
            .map_or(cutoff, |deadline| cutoff.min(deadline)))
    }

    /// Waits within the same window; user cancellation never starts another account or HTTP operation.
    pub(super) async fn wait(&self, cancel: &mut oneshot::Receiver<()>) -> Result<(), Failure> {
        let now = Instant::now();
        self.check(now)?;
        let deadline = self.deadline.expect("signup was presented");
        let next = (now
            + Duration::from_secs(u64::from(bridge::contract::AUTH_XBOX_SIGNUP_RETRY_SECONDS)))
        .min(deadline);
        tokio::select! {
            biased;
            _ = cancel => Err(Failure::Cancelled),
            _ = tokio::time::sleep_until(next) => self.check(Instant::now()),
        }
    }

    /// Rejects expired continuations before their next credential-bearing operation can be polled.
    fn check(&self, now: Instant) -> Result<(), Failure> {
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            Err(Failure::Failed(
                "Xbox profile setup timed out. Try signing in again.",
            ))
        } else {
            Ok(())
        }
    }
}

/// Returns None only when the initial saved account needs interactive OAuth, before any signup continuation.
pub(super) async fn saved_profile(
    store: &AccountStore,
    account: Arc<NativeAccount>,
    selected: Option<&str>,
    sender: &Sender<Message>,
    cancel: &mut oneshot::Receiver<()>,
) -> Result<Option<AccountProfile>, Failure> {
    let first_cutoff = Instant::now() + ACQUISITION_TIMEOUT;
    let generation = tokio::select! {
        biased;
        _ = &mut *cancel => return Err(Failure::Cancelled),
        result = tokio::time::timeout_at(first_cutoff, async {
            loop {
                match store.try_native_lease() {
                    Ok(lease) => return Ok(lease.generation().to_owned()),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => tokio::time::sleep(LEASE_RETRY).await,
                    Err(error) => return Err(error),
                }
            }
        }) => result.map_err(|_| Failure::Failed("Saved account validation timed out. Try again."))?,
    };
    let generation = match generation {
        Ok(generation) => generation,
        Err(error) if selected.is_none() && error.kind() == io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(_) => {
            return Err(Failure::Failed(
                "Could not validate the saved account. Try again.",
            ));
        }
    };
    let mut signup = Signup::default();
    let mut cutoff = first_cutoff;
    loop {
        if Instant::now() >= cutoff {
            return Err(Failure::Failed(
                "Saved account validation timed out. Try again.",
            ));
        }
        let result = tokio::select! {
            biased;
            _ = &mut *cancel => return Err(Failure::Cancelled),
            result = tokio::time::timeout_at(cutoff, Arc::clone(&account).validate_generation(&generation)) =>
                result.map_err(|_| Failure::Failed("Saved account validation timed out. Try again."))?,
        };
        match result {
            Ok(profile) => return Ok(Some(profile)),
            Err(error) => {
                if let Some(challenge) = error
                    .get_ref()
                    .and_then(|cause| cause.downcast_ref::<xbox::AccountCreationRequired>())
                {
                    signup.present(challenge.signup_url(), Instant::now(), sender)?;
                    signup.wait(cancel).await?;
                    cutoff = signup.acquisition_cutoff(Instant::now())?;
                } else if signup.deadline.is_none() && requires_sign_in(&error) {
                    return Ok(None);
                } else {
                    return Err(Failure::Failed(
                        "Could not validate the saved account. Try again.",
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;

//! Independent account-leased optional facets; no presentation read holds presence authority.

use super::*;
use bedrock_services::minecraft::profile::{Gallery, ImageBody, ImageKind};
use std::{future::Future, task::Poll};

enum Failure {
    Deadline,
    Operation(io::Error),
}

impl NativeAccount {
    /// Reacquires and fences the original account before promoting its same proof to Minecraft.
    async fn profile_operation(
        self: &Arc<Self>,
        generation: &str,
        xuid: &str,
    ) -> io::Result<Operation> {
        let mut operation = Arc::clone(self)
            .acquire_verified_for(Some(generation))
            .await?;
        operation.authorize_identity().await?;
        if operation
            .cached
            .as_ref()
            .expect("account owner")
            .identity
            .as_ref()
            .is_none_or(|identity| !identity.current_credential || identity.xuid != xuid)
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "native profile identity changed",
            ));
        }
        operation.promote()?;
        Ok(operation)
    }

    /// Queries one checked retail selection and commits refresh before retaining its projection.
    pub(super) async fn profile_statistics(
        self: &Arc<Self>,
        generation: &str,
        xuid: &str,
    ) -> io::Result<(Option<bridge::ProfileStatistics>, bool)> {
        let mut operation = self.profile_operation(generation, xuid).await?;
        let selection = policy::statistics();
        let response = operation
            .minecraft()?
            .statistics()
            .read(&[xuid], &selection)
            .await;
        operation.persist()?;
        Ok(match optional(response)? {
            Some(batch) => (
                statistics::project(&batch, xuid),
                batch.malformed_users != 0,
            ),
            None => (None, true),
        })
    }

    /// Uses bounded complete pagination and preserves the authenticated response owner.
    pub(super) async fn profile_achievements(
        self: &Arc<Self>,
        generation: &str,
        xuid: &str,
    ) -> io::Result<(Option<bridge::ProfileAchievements>, bool)> {
        let mut operation = self.profile_operation(generation, xuid).await?;
        let response = operation
            .minecraft()?
            .achievements()
            .all(policy::get().achievement_title)
            .await;
        operation.persist()?;
        Ok(match optional(response)? {
            Some(values) => {
                let summary = achievements::project(values, xuid)?;
                let incomplete = summary.is_none();
                (summary, incomplete)
            }
            None => (None, true),
        })
    }

    /// Retains bounded persona bytes only until the existing artwork phase decodes and caches them.
    pub(super) async fn profile_avatar(
        self: &Arc<Self>,
        generation: &str,
        xuid: &str,
    ) -> io::Result<Option<ImageBody>> {
        let mut operation = self.profile_operation(generation, xuid).await?;
        let response = operation
            .minecraft()?
            .profile()
            .image(xuid, ImageKind::Avatar)
            .await;
        operation.persist()?;
        optional(response)
    }

    /// Loads metadata once; generic artwork fetching never receives a Minecraft credential.
    pub(super) async fn profile_gallery(
        self: &Arc<Self>,
        generation: &str,
        xuid: &str,
    ) -> io::Result<Option<Gallery>> {
        let mut operation = self.profile_operation(generation, xuid).await?;
        let response = operation.minecraft()?.profile().gallery(xuid).await;
        operation.persist()?;
        optional(response)
    }
}

/// Keeps optional service failure unavailable while original credential/storage rejection stays fatal.
fn optional<T>(response: Result<T, bedrock_services::Error>) -> io::Result<Option<T>> {
    match response {
        Ok(value) => Ok(Some(value)),
        Err(error) => {
            let error = service_error(error);
            if requires_sign_in(&error) {
                Err(error)
            } else {
                Ok(None)
            }
        }
    }
}

/// Selects only explicit featured permitted art; an empty successful showcase is not an error or count.
pub(super) fn featured(gallery: Gallery) -> (bridge::Artwork, bool) {
    let partial = gallery.malformed_items != 0;
    let artwork = gallery
        .images
        .into_iter()
        .find(|image| {
            image.is_featured == Some(true)
                && crate::remote_images::LAUNCHER_ART.accepts(&image.url)
        })
        .map(|image| bridge::Artwork {
            url: image.url,
            path: String::new(),
        })
        .unwrap_or_default();
    (artwork, partial)
}

impl NativeProfile {
    /// Enriches loaded presentation within the original fetch budget, preserving completed optional facets.
    pub(crate) async fn enrich(&mut self, deadline: tokio::time::Instant) -> io::Result<()> {
        match self.enrich_inner(deadline).await {
            Err(Failure::Deadline) => {
                self.partial = true;
                Ok(())
            }
            Err(Failure::Operation(error)) => Err(error),
            Ok(()) => Ok(()),
        }
    }

    /// Commits each result separately; a later timeout cannot discard the loaded overview or earlier facets.
    async fn enrich_inner(&mut self, deadline: tokio::time::Instant) -> Result<(), Failure> {
        let (statistics, incomplete) = before(
            deadline,
            self.account
                .profile_statistics(&self.generation, &self.profile.xuid),
        )
        .await?;
        self.partial |= incomplete || statistics.is_none();
        self.profile.statistics = statistics;
        let (achievements, incomplete) = before(
            deadline,
            self.account
                .profile_achievements(&self.generation, &self.profile.xuid),
        )
        .await?;
        self.partial |= incomplete || achievements.is_none();
        self.profile.achievements = achievements;
        self.avatar = before(
            deadline,
            self.account
                .profile_avatar(&self.generation, &self.profile.xuid),
        )
        .await?;
        self.partial |= self.avatar.is_none();
        if let Some(gallery) = before(
            deadline,
            self.account
                .profile_gallery(&self.generation, &self.profile.xuid),
        )
        .await?
        {
            let (artwork, incomplete) = featured(gallery);
            self.partial |= incomplete;
            self.profile.featured_screenshot_error = incomplete && artwork.url.is_empty();
            self.profile.featured_screenshot = artwork;
        } else {
            self.partial = true;
        }
        Ok(())
    }
}

/// Uses the caller's one original deadline, checking both admission and a delayed ready completion.
async fn before<T>(
    deadline: tokio::time::Instant,
    operation: impl Future<Output = io::Result<T>>,
) -> Result<T, Failure> {
    let expires = tokio::time::sleep_until(deadline);
    tokio::pin!(operation, expires);
    std::future::poll_fn(|cx| {
        if tokio::time::Instant::now() >= deadline || expires.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(Failure::Deadline));
        }
        match operation.as_mut().poll(cx) {
            Poll::Ready(Err(error)) => Poll::Ready(Err(Failure::Operation(error))),
            Poll::Ready(Ok(_)) if tokio::time::Instant::now() >= deadline => {
                Poll::Ready(Err(Failure::Deadline))
            }
            Poll::Ready(Ok(value)) => Poll::Ready(Ok(value)),
            Poll::Pending => Poll::Pending,
        }
    })
    .await
}

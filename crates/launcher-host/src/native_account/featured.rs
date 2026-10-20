//! Featured metadata and population reads on the selected account's existing SDK owner.

mod join;
pub(crate) mod projection;
#[cfg(test)]
mod tests;

use super::prepare::check_preparation_deadline;
use super::{read_identity::ReadIdentity, *};
use bedrock_services::minecraft::gatherings::{ExperiencePlayerCount, SearchFilter};
use tokio::time::Instant;
use uuid::Uuid;

/// One explicit experience selection from an original verified account's catalog.
/// The service must still resolve and authorize its actual connection when selected.
#[derive(Clone)]
pub struct NativeGatheringSelection {
    pub(super) identity: ReadIdentity,
    pub(super) experience: Uuid,
}

/// Original presentation rows and their account-bound allocation selections.
#[derive(Clone)]
pub(crate) struct FeaturedEntries {
    pub(crate) servers: Vec<bridge::FeaturedServer>,
    pub(crate) gatherings: Vec<Option<NativeGatheringSelection>>,
}

impl FeaturedEntries {
    /// Applies actual source-ordered population values without changing the layout or selections.
    pub(crate) fn apply_counts(&mut self, counts: &[ExperiencePlayerCount]) {
        projection::counts(&mut self.servers, counts);
    }
}

/// A completed service result retains its verified lease through final publication.
pub(crate) struct NativeFeatured {
    operation: Operation,
    pub(crate) identity: FeaturedIdentity,
    pub(crate) entries: FeaturedEntries,
}

/// Identity of a cached layout, independent of any later population read.
#[derive(Clone)]
pub(crate) struct FeaturedIdentity(ReadIdentity);

impl FeaturedIdentity {
    /// Defers a cached publication during ordinary credential contention without discarding it.
    pub(crate) async fn wait_current(&self) -> bool {
        self.0.wait_current().await
    }
}

/// Population results retain the same account lease until their overlay is published.
pub(crate) struct NativeFeaturedCounts {
    operation: Operation,
    identity: FeaturedIdentity,
    pub(crate) counts: Vec<ExperiencePlayerCount>,
}

impl NativeFeaturedCounts {
    /// Checks this actual population operation without reacquiring its protected account.
    pub(crate) fn current(&self) -> bool {
        self.identity.0.current_operation(&self.operation)
    }
}

impl NativeFeatured {
    /// Checks this original account while the completed read still holds its actual lease.
    pub(crate) fn current(&self) -> bool {
        self.identity.0.current_operation(&self.operation)
    }
}

impl NativeAccount {
    /// Reads one source-ordered catalog page without allocating any experience server.
    pub(crate) async fn read_featured(
        self: Arc<Self>,
        deadline: Instant,
    ) -> io::Result<NativeFeatured> {
        tokio::time::timeout_at(deadline, async {
            check_preparation_deadline(deadline)?;
            let mut operation = self.acquire_verified().await?;
            check_preparation_deadline(deadline)?;
            operation.promote()?;
            let identity = ReadIdentity::capture(&operation);
            let mut gatherings = operation
                .minecraft()?
                .gatherings()
                .await
                .map_err(service_error)?;
            check_preparation_deadline(deadline)?;
            let result = gatherings.search_items(&SearchFilter::default()).await;
            operation.persist()?;
            check_preparation_deadline(deadline)?;
            let rows = projection::rows(result.map_err(service_error)?.items);
            let (servers, gatherings) = rows
                .into_iter()
                .map(|(server, experience)| {
                    let selection = experience.map(|experience| NativeGatheringSelection {
                        identity: identity.clone(),
                        experience,
                    });
                    (server, selection)
                })
                .unzip();
            check_preparation_deadline(deadline)?;
            Ok(NativeFeatured {
                operation,
                identity: FeaturedIdentity(identity),
                entries: FeaturedEntries {
                    servers,
                    gatherings,
                },
            })
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "featured catalog timed out"))?
    }

    /// Reads only populations under the cached layout's original account identity.
    /// A completed failed refresh preserves the SDK's previously returned counts.
    pub(crate) async fn read_featured_counts(
        self: Arc<Self>,
        identity: &FeaturedIdentity,
        deadline: Instant,
    ) -> io::Result<NativeFeaturedCounts> {
        if !Arc::ptr_eq(&self, &identity.0.account) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "featured account changed",
            ));
        }
        tokio::time::timeout_at(deadline, async {
            check_preparation_deadline(deadline)?;
            let mut operation = self
                .acquire_verified_for(Some(&identity.0.generation))
                .await?;
            operation.promote()?;
            check_preparation_deadline(deadline)?;
            if !identity.0.current_operation(&operation) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "featured account changed",
                ));
            }
            let mut gatherings = operation
                .minecraft()?
                .gatherings()
                .await
                .map_err(service_error)?;
            check_preparation_deadline(deadline)?;
            let counts = match gatherings.player_counts().await {
                Ok(counts) => counts,
                Err(_) => gatherings.cached_player_counts().to_vec(),
            };
            operation.persist()?;
            check_preparation_deadline(deadline)?;
            if !identity.0.current_operation(&operation) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "featured account changed",
                ));
            }
            Ok(NativeFeaturedCounts {
                operation,
                identity: identity.clone(),
                counts,
            })
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "featured populations timed out"))?
    }
}

//! The original selected native profile owner and its retained publication authority.
use super::*;

#[derive(Clone, Default)]
pub(in crate::launcher_account) struct Source(
    pub(in crate::launcher_account) Option<Arc<crate::native_account::NativeAccount>>,
);

impl Source {
    /// Compares the captured account without acquiring credentials or blocking presentation.
    pub(super) fn same_account(&self, current: &Self) -> bool {
        match (&self.0, &current.0) {
            (Some(first), Some(second)) => Arc::ptr_eq(first, second),
            (None, None) => true,
            _ => false,
        }
    }
}

pub(super) struct Loaded(crate::native_account::NativeProfile);

impl Source {
    /// An absent selected account remains unavailable rather than borrowing another identity.
    pub(super) async fn fetch(&self) -> Result<Loaded, ()> {
        Arc::clone(self.0.as_ref().ok_or(())?)
            .read_profile()
            .await
            .map(Loaded)
            .map_err(|_| ())
    }
}

impl Loaded {
    /// Keeps optional facets within the original remaining request budget.
    pub(super) async fn enrich(&mut self, deadline: tokio::time::Instant) -> Result<(), ()> {
        self.0.enrich(deadline).await.map_err(|_| ())
    }
    /// Caches already-acquired persona bytes through the same image owner.
    pub(super) fn cache_avatar(&mut self, images: &crate::remote_images::ImageDirectory) {
        self.0.cache_avatar(images);
    }
    /// Borrows presentation while retaining its original native operation.
    pub(super) fn profile_mut(&mut self) -> &mut Profile {
        &mut self.0.profile
    }
    /// Reports supported partial reads truthfully.
    pub(super) fn partial(&self) -> bool {
        self.0.partial
    }
    /// Rechecks the original generation after all artwork awaits.
    pub(super) async fn into_current(self) -> Result<Profile, ()> {
        self.0.into_current().await.ok_or(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn missing_native_profile_owner_remains_unavailable() {
        assert!(Source(None).fetch().await.is_err());
    }
}

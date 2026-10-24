//! The invite picker retains its selected native account through fetch and artwork.
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

pub(super) struct Loaded(crate::native_account::NativePeople);

impl Source {
    /// Reads only the selected concrete account.
    pub(super) async fn fetch(&self) -> Result<Loaded, ()> {
        Arc::clone(self.0.as_ref().ok_or(())?)
            .read_people()
            .await
            .map(Loaded)
            .map_err(|_| ())
    }
}

impl Loaded {
    /// Borrows the same rows for the bounded image cache.
    pub(super) fn people_mut(&mut self) -> &mut [bridge::Person] {
        &mut self.0.people
    }
    /// Rechecks authority after artwork releases asynchronous I/O.
    pub(super) async fn into_current(self) -> Result<Vec<bridge::Person>, ()> {
        self.0.into_current().await.ok_or(())
    }
}

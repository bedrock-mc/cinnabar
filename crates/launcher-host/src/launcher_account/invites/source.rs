//! The invite picker retains its selected account through fetch and artwork.

use super::*;

#[derive(Clone, Default)]
pub(in crate::launcher_account) enum Source {
    #[default]
    Core,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    Native(Option<Arc<crate::native_account::NativeAccount>>),
}

pub(super) enum Loaded {
    Core(Vec<bridge::Person>),
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    Native(crate::native_account::NativePeople),
}

impl Source {
    /// Never substitutes an unrelated Go account when the selected native owner is missing.
    pub(super) async fn fetch(&self, socket_dir: Option<&std::path::Path>) -> Result<Loaded, ()> {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        if let Self::Native(account) = self {
            return match account {
                Some(account) => Arc::clone(account)
                    .read_people()
                    .await
                    .map(Loaded::Native)
                    .map_err(|_| ()),
                None => Err(()),
            };
        }
        bridge::list_people(
            super::super::workers::require_core_directory(socket_dir).map_err(|_| ())?,
        )
        .await
        .map(Loaded::Core)
        .map_err(|_| ())
    }

    /// Compares the original concrete account without leases or authentication on the frame.
    pub(super) fn same_account(&self, current: &Self) -> bool {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        match (self, current) {
            (Self::Core, Self::Core) => true,
            (Self::Native(first), Self::Native(second)) => match (first, second) {
                (Some(first), Some(second)) => Arc::ptr_eq(first, second),
                (None, None) => true,
                _ => false,
            },
            _ => false,
        }
        #[cfg(not(all(feature = "native-networking", not(target_arch = "wasm32"))))]
        {
            let (Self::Core, Self::Core) = (self, current);
            true
        }
    }
}

impl Loaded {
    /// Borrows the same bounded rows for the existing image cache.
    pub(super) fn people_mut(&mut self) -> &mut [bridge::Person] {
        match self {
            Self::Core(people) => people,
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            Self::Native(people) => &mut people.people,
        }
    }

    /// Rechecks native authority only after artwork has released its asynchronous I/O.
    pub(super) async fn into_current(self) -> Result<Vec<bridge::Person>, ()> {
        match self {
            Self::Core(people) => Ok(people),
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            Self::Native(people) => people.into_current().await.ok_or(()),
        }
    }
}

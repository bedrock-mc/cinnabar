//! One actual request source selected before the existing profile worker starts.

use super::*;

/// Keeps the worker on its caller-selected account source through every read and artwork await.
#[derive(Clone, Default)]
pub(in crate::launcher_account) enum Source {
    #[default]
    Core,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    Native(Option<Arc<crate::native_account::NativeAccount>>),
}

/// Retains native publication authority alongside the shared presentation DTO.
pub(super) enum Loaded {
    Core(Profile),
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    Native(crate::native_account::NativeProfile),
}

impl Source {
    /// Fetches only the selected source; a missing native account never queries another Go account.
    pub(super) async fn fetch(&self, socket_dir: Option<&std::path::Path>) -> Result<Loaded, ()> {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        if let Self::Native(account) = self {
            return match account {
                Some(account) => Arc::clone(account)
                    .read_profile()
                    .await
                    .map(Loaded::Native)
                    .map_err(|_| ()),
                None => Err(()),
            };
        }
        bridge::profile(super::super::workers::require_core_directory(socket_dir).map_err(|_| ())?)
            .await
            .map(Loaded::Core)
            .map_err(|_| ())
    }

    /// Checks the original presentation authority without account files, acquisition or a frame-thread wait.
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
    /// Retains the loaded overview while native optional facets use only the original remaining budget.
    pub(super) async fn enrich(&mut self, deadline: tokio::time::Instant) -> Result<(), ()> {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        if let Self::Native(result) = self {
            result.enrich(deadline).await.map_err(|_| ())?;
        }
        #[cfg(not(all(feature = "native-networking", not(target_arch = "wasm32"))))]
        let _ = deadline;
        Ok(())
    }

    /// Publishes already-acquired native persona bytes through the same bounded image owner.
    pub(super) fn cache_avatar(&mut self, images: &crate::remote_images::ImageDirectory) {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        if let Self::Native(result) = self {
            result.cache_avatar(images);
        }
        #[cfg(not(all(feature = "native-networking", not(target_arch = "wasm32"))))]
        let _ = images;
    }

    /// Borrows the existing presentation DTO for the shared bounded artwork cache.
    pub(super) fn profile_mut(&mut self) -> &mut Profile {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        match self {
            Self::Core(profile) => profile,
            Self::Native(result) => &mut result.profile,
        }
        #[cfg(not(all(feature = "native-networking", not(target_arch = "wasm32"))))]
        {
            let Self::Core(profile) = self;
            profile
        }
    }

    /// Distinguishes partial supported native reads without labeling missing counts as zero.
    pub(super) fn partial(&self) -> bool {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        match self {
            Self::Core(_) => false,
            Self::Native(result) => result.partial,
        }
        #[cfg(not(all(feature = "native-networking", not(target_arch = "wasm32"))))]
        {
            false
        }
    }

    /// Rechecks the original native generation after all artwork awaits before publishing.
    pub(super) async fn into_current(self) -> Result<Profile, ()> {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        match self {
            Self::Core(profile) => Ok(profile),
            Self::Native(result) => result.into_current().await.ok_or(()),
        }
        #[cfg(not(all(feature = "native-networking", not(target_arch = "wasm32"))))]
        {
            let Self::Core(profile) = self;
            Ok(profile)
        }
    }
}

#[cfg(all(test, unix, feature = "native-networking", not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    /// An absent native selection cannot disclose another account's core profile.
    #[tokio::test]
    async fn missing_native_profile_owner_never_falls_back_to_core() {
        let directory = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(directory.path().join("control.sock")).unwrap();
        listener.set_nonblocking(true).unwrap();
        assert!(
            Source::Native(None)
                .fetch(Some(directory.path()))
                .await
                .is_err()
        );
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}

//! Delivers render-world admission failures to the pack picker and native fallback.

use super::{loader::PackGpu, plugin::LoadedShaderPack};
use bevy::prelude::*;
use render_scene::shaderpack::ShaderPackStatus;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

/// The last failed view; it cannot retain a growing list of packs or view sizes.
#[derive(Default)]
struct Pending {
    available: AtomicBool,
    failure: Mutex<Option<(Arc<PackGpu>, String)>>,
}

/// One shared mailbox installed in both worlds, with no per-frame extraction or allocation.
#[derive(Resource, Clone, Default)]
pub(super) struct ViewFailures(Arc<Pending>);

impl ViewFailures {
    /// Publishes an admission error with the exact pack allocation it came from.
    pub(super) fn record(&self, pack: Arc<PackGpu>, reason: String) {
        *self
            .0
            .failure
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some((pack, reason));
        self.0.available.store(true, Ordering::Release);
    }

    /// Takes and releases a pending failure, including after the player selected Off.
    fn take(&self) -> Option<(Arc<PackGpu>, String)> {
        if !self.0.available.swap(false, Ordering::Acquire) {
            return None;
        }
        self.0
            .failure
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
    }
}

/// Rejects only the still-shown pack; late errors cannot fail a replacement or retain Off resources.
pub(super) fn receive_view_failures(
    failures: Res<ViewFailures>,
    mut loaded: ResMut<LoadedShaderPack>,
    mut status: ResMut<ShaderPackStatus>,
) {
    let Some((failed, reason)) = failures.take() else {
        return;
    };
    if loaded
        .0
        .as_ref()
        .is_some_and(|pack| Arc::ptr_eq(pack, &failed))
        && let ShaderPackStatus::Ready(path) = &*status
    {
        let pack = path.clone();
        loaded.0 = None;
        *status = ShaderPackStatus::Failed { pack, reason };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn failed_view_admission_reaches_the_picker_and_releases_the_pack() {
        let mut world = super::super::plugin::native_tests::ready_world();
        world.init_resource::<ViewFailures>();
        world.insert_resource(ShaderPackStatus::Ready("fixture".into()));
        let pack = world
            .resource::<LoadedShaderPack>()
            .0
            .as_ref()
            .unwrap()
            .clone();
        world
            .resource::<ViewFailures>()
            .record(pack, "texture exceeds device limit".into());
        world.run_system_once(receive_view_failures).unwrap();
        assert!(world.resource::<LoadedShaderPack>().0.is_none());
        assert!(
            matches!(world.resource::<ShaderPackStatus>(), ShaderPackStatus::Failed { reason, .. } if reason == "texture exceeds device limit")
        );
    }

    #[test]
    fn late_view_failures_are_drained_after_off_or_a_replacement() {
        let mut world = super::super::plugin::native_tests::ready_world();
        world.init_resource::<ViewFailures>();
        let first = world
            .resource::<LoadedShaderPack>()
            .0
            .as_ref()
            .unwrap()
            .clone();
        let mut replacement = super::super::plugin::native_tests::ready_world();
        let second = replacement
            .remove_resource::<LoadedShaderPack>()
            .unwrap()
            .0
            .unwrap();
        for shown in [None, Some(second)] {
            world.resource_mut::<LoadedShaderPack>().0 = shown.clone();
            world.insert_resource(ShaderPackStatus::Idle);
            world
                .resource::<ViewFailures>()
                .record(first.clone(), "late".into());
            world.run_system_once(receive_view_failures).unwrap();
            assert!(world.resource::<ViewFailures>().take().is_none());
            assert_eq!(
                *world.resource::<ShaderPackStatus>(),
                ShaderPackStatus::Idle
            );
            assert_eq!(
                world.resource::<LoadedShaderPack>().0.is_some(),
                shown.is_some()
            );
        }
    }
}

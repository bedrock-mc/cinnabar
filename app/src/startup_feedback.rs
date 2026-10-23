//! Native startup feedback, retired only after a completed destination-interface frame.

use bevy::prelude::*;
use launcher::startup::{Message, Stage};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[cfg(target_os = "android")]
static OBSERVER: std::sync::OnceLock<Observer> = std::sync::OnceLock::new();

#[cfg(target_os = "android")]
struct Observer {
    message: fn(Message, u64),
    generation: fn() -> u64,
}

/// Connects the activity's overlay before native startup begins.
#[cfg(target_os = "android")]
pub fn observe(observer: fn(Message, u64), generation: fn() -> u64) {
    let _ = OBSERVER.set(Observer {
        message: observer,
        generation,
    });
}

/// Keeps the desktop feedback process alive through startup, failure, and normal teardown.
#[derive(Clone, Default, Resource)]
pub(crate) struct Feedback {
    desktop: Option<Arc<bootstrap_ui::startup::Controller>>,
    generation: Arc<AtomicU64>,
}

impl Feedback {
    /// Starts after first-run consent and before carriers, fonts, or the graphics device load.
    pub(crate) fn start() -> Self {
        let desktop = if cfg!(target_os = "android")
            || std::env::var_os("CINNABAR_HIDDEN_WINDOW").is_some()
        {
            None
        } else {
            let size = bevy::window::WindowResolution::default();
            match bootstrap_ui::startup::Controller::start([size.width(), size.height()]) {
                Ok(controller) => Some(Arc::new(controller)),
                Err(error) => {
                    warn!("startup feedback unavailable: {error:#}");
                    None
                }
            }
        };
        let feedback = Self {
            desktop,
            ..Default::default()
        };
        feedback.progress(Stage::LoadingAssets);
        feedback
    }

    /// Reports entered work and checks cancellation at startup's existing ownership boundaries.
    pub(crate) fn progress(&self, stage: Stage) {
        if let Some(desktop) = &self.desktop {
            desktop.progress(stage);
        }
        Self::notify(Message::Progress { stage }, lifecycle_generation());
    }

    /// Returns whether the user closed feedback before the main loop became available.
    pub(crate) fn cancelled(&self) -> bool {
        self.desktop
            .as_ref()
            .is_some_and(|desktop| desktop.cancelled())
    }

    /// Routes native activity messages without letting platform code own client scheduling.
    fn notify(message: Message, generation: u64) {
        #[cfg(target_os = "android")]
        if let Some(observer) = OBSERVER.get() {
            (observer.message)(message, generation);
        }
        #[cfg(not(target_os = "android"))]
        let _ = (message, generation);
    }
}

/// Reads the activity delivery lifetime, which can change without a native-window replacement.
fn lifecycle_generation() -> u64 {
    #[cfg(target_os = "android")]
    if let Some(observer) = OBSERVER.get() {
        return (observer.generation)();
    }
    0
}

/// Tracks the main thread's native handle before any pending handoff is consumed.
pub(crate) fn synchronize_surface(
    presentation: Res<render_scene::startup_presentation::StartupPresentation>,
    feedback: Res<Feedback>,
    windows: Query<
        (Entity, Ref<bevy::window::RawHandleWrapper>),
        With<bevy::window::PrimaryWindow>,
    >,
    mut lifetime: Local<SurfaceLifetime>,
) {
    let current = windows
        .single()
        .ok()
        .map(|(entity, handle)| (entity, handle.last_changed()));
    let generation = lifecycle_generation();
    feedback.generation.store(generation, Ordering::Relaxed);
    lifetime.synchronize(&presentation, current, generation);
}

#[derive(Default)]
pub(crate) struct SurfaceLifetime {
    previous: Option<SurfaceStamp>,
}

#[derive(PartialEq, Eq)]
struct SurfaceStamp {
    handle: Option<(Entity, bevy::ecs::change_detection::Tick)>,
    generation: u64,
}

impl SurfaceLifetime {
    /// Requires fresh content after either native-surface or activity-delivery replacement.
    fn synchronize(
        &mut self,
        presentation: &render_scene::startup_presentation::StartupPresentation,
        handle: Option<(Entity, bevy::ecs::change_detection::Tick)>,
        generation: u64,
    ) {
        let current = SurfaceStamp { handle, generation };
        if self.previous.as_ref() != Some(&current) {
            presentation.replace_content_surface(current.handle.is_some());
            self.previous = Some(current);
        }
    }
}

/// Hands off after actual UI content, rather than merely counting cleared swapchain images.
pub(crate) fn finish(
    feedback: Res<Feedback>,
    presentation: Res<render_scene::startup_presentation::StartupPresentation>,
    mut sent: Local<Option<u64>>,
    mut exit: MessageWriter<AppExit>,
) {
    if feedback.cancelled() {
        exit.write(AppExit::Success);
    } else if let Some(epoch) = presentation.content_epoch()
        && *sent != Some(epoch)
        && presentation.has_presented_content()
    {
        *sent = Some(epoch);
        if let Some(desktop) = &feedback.desktop {
            desktop.ready();
        }
        Feedback::notify(Message::Ready, feedback.generation.load(Ordering::Relaxed));
    }
}

/// Revokes an expensive ownership boundary before entry and prefers quiet cancellation on exit.
pub(crate) fn while_active<T>(
    cancelled: impl Fn() -> bool,
    work: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<Option<T>> {
    if cancelled() {
        return Ok(None);
    }
    let result = work();
    if cancelled() {
        Ok(None)
    } else {
        result.map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::{SurfaceLifetime, while_active};
    use std::cell::Cell;

    #[test]
    fn cancelled_preparation_never_admits_a_connection() {
        let admitted = Cell::new(false);
        let result = while_active(
            || true,
            || {
                admitted.set(true);
                Ok(())
            },
        )
        .unwrap();
        assert!(result.is_none());
        assert!(!admitted.get());
    }

    #[test]
    fn cancellation_during_preflight_is_quiet_even_when_the_endpoint_fails() {
        let cancelled = Cell::new(false);
        let result = while_active(
            || cancelled.get(),
            || -> anyhow::Result<()> {
                cancelled.set(true);
                anyhow::bail!("endpoint failed");
            },
        )
        .unwrap();
        assert!(result.is_none());
    }
    #[test]
    fn resume_retries_dropped_readiness_when_the_native_surface_survives() {
        use bevy::{ecs::change_detection::Tick, prelude::Entity};
        use render_scene::startup_presentation::StartupPresentation;
        let presentation = StartupPresentation::default();
        let mut lifetime = SurfaceLifetime::default();
        let handle = Some((Entity::PLACEHOLDER, Tick::new(1)));
        lifetime.synchronize(&presentation, handle, 1);
        let queued = presentation.content_epoch().unwrap();
        presentation.record_content_frame(queued, std::time::Instant::now());
        assert!(presentation.has_presented_content());
        lifetime.synchronize(&presentation, handle, 2);
        lifetime.synchronize(&presentation, handle, 3);
        let resumed = presentation.content_epoch().unwrap();
        assert_ne!(queued, resumed);
        assert!(!presentation.has_presented_content());
        presentation.record_content_frame(queued, std::time::Instant::now());
        assert!(!presentation.has_presented_content());
        presentation.record_content_frame(resumed, std::time::Instant::now());
        assert!(presentation.has_presented_content());
        lifetime.synchronize(&presentation, handle, 3);
        assert_eq!(presentation.content_epoch(), Some(resumed));
        assert!(presentation.has_presented_content());
    }
}

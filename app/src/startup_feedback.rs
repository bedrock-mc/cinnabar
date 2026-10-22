//! Native startup feedback, retired only after a completed destination-interface frame.

use bevy::prelude::*;
use launcher::startup::{Message, Stage};
use std::sync::Arc;

#[cfg(target_os = "android")]
static OBSERVER: std::sync::OnceLock<fn(Message)> = std::sync::OnceLock::new();

/// Connects the activity's overlay before native startup begins.
#[cfg(target_os = "android")]
pub fn observe(observer: fn(Message)) {
    let _ = OBSERVER.set(observer);
}

/// Keeps the desktop feedback process alive through startup, failure, and normal teardown.
#[derive(Clone, Default, Resource)]
pub(crate) struct Feedback {
    desktop: Option<Arc<bootstrap_ui::startup::Controller>>,
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
        let feedback = Self { desktop };
        feedback.progress(Stage::LoadingAssets);
        feedback
    }

    /// Reports entered work and checks cancellation at startup's existing ownership boundaries.
    pub(crate) fn progress(&self, stage: Stage) {
        if let Some(desktop) = &self.desktop {
            desktop.progress(stage);
        }
        Self::notify(Message::Progress { stage });
    }

    /// Returns whether the user closed feedback before the main loop became available.
    pub(crate) fn cancelled(&self) -> bool {
        self.desktop
            .as_ref()
            .is_some_and(|desktop| desktop.cancelled())
    }

    /// Routes native activity messages without letting platform code own client scheduling.
    fn notify(message: Message) {
        #[cfg(target_os = "android")]
        if let Some(observer) = OBSERVER.get() {
            observer(message);
        }
        #[cfg(not(target_os = "android"))]
        let _ = message;
    }
}

/// Tracks the main thread's native handle before any pending handoff is consumed.
pub(crate) fn synchronize_surface(
    presentation: Res<render_scene::startup_presentation::StartupPresentation>,
    windows: Query<
        (Entity, Ref<bevy::window::RawHandleWrapper>),
        With<bevy::window::PrimaryWindow>,
    >,
    mut previous: Local<Option<(Entity, bevy::ecs::change_detection::Tick)>>,
) {
    let current = windows
        .single()
        .ok()
        .map(|(entity, handle)| (entity, handle.last_changed()));
    if current != *previous {
        *previous = current;
        presentation.replace_content_surface(current.is_some());
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
        Feedback::notify(Message::Ready);
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
    use super::while_active;
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
}

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

/// Hands off after actual UI content, rather than merely counting cleared swapchain images.
pub(crate) fn finish(
    feedback: Res<Feedback>,
    presentation: Res<render_scene::startup_presentation::StartupPresentation>,
    mut sent: Local<bool>,
    mut exit: MessageWriter<AppExit>,
) {
    if *sent {
        return;
    }
    if feedback.cancelled() {
        *sent = true;
        exit.write(AppExit::Success);
    } else if presentation.has_presented_content() {
        *sent = true;
        if let Some(desktop) = &feedback.desktop {
            desktop.ready();
        }
        Feedback::notify(Message::Ready);
    }
}

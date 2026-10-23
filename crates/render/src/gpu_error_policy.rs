//! Terminal GPU failures passed to the application for its restart error screen.

use bevy::render::error_handler::{ErrorType, RenderError, RenderErrorHandler, RenderErrorPolicy};

/// Hands terminal GPU errors to the main world; other errors keep Bevy's policy.
/// Install after the render plugin so this handler replaces the default.
pub fn install_error_policy(app: &mut bevy::app::App) {
    app.init_resource::<GpuFailure>()
        .insert_resource(RenderErrorHandler(error_policy));
}

/// The first terminal GPU failure; the app stops and shows the driver's report.
#[derive(bevy::prelude::Resource, Default)]
pub struct GpuFailure(Option<String>);

impl GpuFailure {
    /// The driver's description once rendering cannot safely continue.
    pub fn message(&self) -> Option<&str> {
        self.0.as_deref()
    }

    /// Keeps the first failure; Bevy repeats a stopped renderer's error every frame.
    pub fn record(&mut self, description: &str) {
        self.0.get_or_insert_with(|| description.to_owned());
    }
}

/// Stops unsafe rendering while the main world reports the first terminal GPU error.
fn error_policy(
    error: &RenderError,
    main_world: &mut bevy::ecs::world::World,
    render_world: &mut bevy::ecs::world::World,
) -> RenderErrorPolicy {
    match error.ty {
        ErrorType::DeviceLost | ErrorType::OutOfMemory => {
            let description = if error.description.is_empty() {
                match error.ty {
                    ErrorType::OutOfMemory => "uncaptured GPU out-of-memory error",
                    _ => "GPU device lost",
                }
            } else {
                &error.description
            };
            main_world
                .get_resource_or_init::<GpuFailure>()
                .record(description);
            RenderErrorPolicy::StopRendering
        }
        _ => (RenderErrorHandler::default().0)(error, main_world, render_world),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        app::AppExit,
        ecs::{message::Messages, world::World},
    };

    /// Both terminal GPU errors stop without a bare exit and keep the first driver report.
    #[test]
    fn terminal_gpu_errors_stop_rendering_and_record_the_first_reason() {
        for ty in [ErrorType::DeviceLost, ErrorType::OutOfMemory] {
            let (mut main_world, mut render_world) = (World::new(), World::new());
            main_world.init_resource::<Messages<AppExit>>();
            for description in ["first driver report", "repeated"] {
                let error = RenderError {
                    ty,
                    description: description.into(),
                    source: None,
                };
                let policy = error_policy(&error, &mut main_world, &mut render_world);
                assert!(matches!(policy, RenderErrorPolicy::StopRendering), "{ty:?}");
            }
            assert!(main_world.resource::<Messages<AppExit>>().is_empty());
            assert_eq!(
                main_world.resource::<GpuFailure>().message(),
                Some("first driver report")
            );
        }
    }

    /// Unhandled GPU errors retain Bevy's default error exit.
    #[test]
    fn other_errors_keep_bevys_default_policy() {
        for ty in [ErrorType::Validation, ErrorType::Internal] {
            let (mut main_world, mut render_world) = (World::new(), World::new());
            main_world.init_resource::<Messages<AppExit>>();
            let error = RenderError {
                ty,
                description: "driver report".into(),
                source: None,
            };
            let policy = error_policy(&error, &mut main_world, &mut render_world);
            assert!(matches!(policy, RenderErrorPolicy::StopRendering));
            assert!(!main_world.resource::<Messages<AppExit>>().is_empty());
            assert!(main_world.get_resource::<GpuFailure>().is_none());
        }
    }
}

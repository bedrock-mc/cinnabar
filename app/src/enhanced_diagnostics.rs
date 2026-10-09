//! Explicit admission for bounded, hidden development captures.
use crate::args::ClientArgs;
use anyhow::{Result, ensure};

const MAX_FRAME_CAP: u32 = 30;
const MAX_LIFETIME: u64 = 90;

const DIAGNOSTIC_ENV: &str = "CINNABAR_ENHANCED_DIAGNOSTIC";

/// Rejects unbounded diagnostic launches before native application or GPU creation.
pub(crate) fn configure(args: &ClientArgs) -> Result<Option<DiagnosticBudget>> {
    let Some(value) = std::env::var_os(DIAGNOSTIC_ENV) else {
        return Ok(None);
    };
    ensure!(value == "1", "invalid Enhanced diagnostic request");
    ensure!(
        crate::developer_control::hidden_window_requested(),
        "Enhanced diagnostics require a hidden developer client"
    );
    ensure!(
        std::env::var_os(developer_control::ENDPOINT_ENV).is_some(),
        "Enhanced diagnostics require the developer control endpoint"
    );
    ensure!(
        bounded_request(args),
        "Enhanced diagnostics require explicit Enhanced mode, a frame cap up to 30 and a timed exit up to 90 seconds"
    );
    let viewport = std::env::var(developer_control::WINDOW_SIZE_ENV)
        .ok()
        .and_then(|value| developer_control::parse_size(&value))
        .ok_or_else(|| anyhow::anyhow!("Enhanced diagnostics require an explicit viewport"))?;
    render_model::enable_enhanced_diagnostics(viewport).map_err(anyhow::Error::msg)?;
    Ok(Some(DiagnosticBudget {
        deadline: std::time::Instant::now()
            + std::time::Duration::from_secs(args.acceptance_seconds.unwrap()),
        frame_rate: render_model::FrameRate::from_hz(args.frame_cap.unwrap()).unwrap(),
    }))
}

/// Requires a nonzero frame budget and a short explicit diagnostic session.
fn bounded_request(args: &ClientArgs) -> bool {
    args.render_mode == Some(ui::RenderMode::Enhanced)
        && args
            .frame_cap
            .is_some_and(|cap| (1..=MAX_FRAME_CAP).contains(&cap))
        && args
            .acceptance_seconds
            .is_some_and(|seconds| (1..=MAX_LIFETIME).contains(&seconds))
}

/// Launch-time limits retained across startup, menus and fixed-clock recordings.
#[derive(bevy::prelude::Resource)]
pub(crate) struct DiagnosticBudget {
    deadline: std::time::Instant,
    frame_rate: render_model::FrameRate,
}

/// Installs the launch deadline independently of joins and acceptance readiness.
pub(crate) fn install(app: &mut bevy::prelude::App, budget: Option<DiagnosticBudget>) {
    let Some(budget) = budget else {
        return;
    };
    app.world_mut()
        .resource_mut::<crate::frame_pacing::FramePacingRuntime>()
        .require_limit(budget.frame_rate);
    install_deadline(app, budget);
}

/// Uses the monotonic process clock rather than the clock that recordings replace.
fn install_deadline(app: &mut bevy::prelude::App, budget: DiagnosticBudget) {
    app.insert_resource(budget)
        .add_systems(bevy::prelude::Update, expire);
}

/// Bounds resized surfaces and requests orderly exit when the launch budget expires.
fn expire(
    budget: bevy::prelude::Res<DiagnosticBudget>,
    mut exit: bevy::prelude::MessageWriter<bevy::app::AppExit>,
    mut windows: bevy::prelude::Query<&mut bevy::window::Window>,
) {
    for mut window in &mut windows {
        let size = window.resolution.physical_size().to_array();
        let bounded: [u32; 2] = std::array::from_fn(|axis| {
            size[axis].clamp(1, render_model::ENHANCED_DIAGNOSTIC_MAX_VIEWPORT[axis])
        });
        if size != bounded {
            window
                .resolution
                .set_physical_resolution(bounded[0], bounded[1]);
        }
    }
    if std::time::Instant::now() >= budget.deadline {
        exit.write(bevy::app::AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_limits_exit_without_a_ready_world() {
        use bevy::prelude::*;
        let mut app = App::new();
        app.add_message::<bevy::app::AppExit>();
        let mut time = Time::<Real>::default();
        time.advance_by(std::time::Duration::ZERO);
        app.insert_resource(time);
        install_deadline(
            &mut app,
            DiagnosticBudget {
                deadline: std::time::Instant::now(),
                frame_rate: render_model::FrameRate::from_hz(MAX_FRAME_CAP).unwrap(),
            },
        );
        app.update();
        assert_eq!(
            app.world().resource::<Messages<bevy::app::AppExit>>().len(),
            1
        );
    }

    #[test]
    fn diagnostic_limits_bound_later_viewport_resizes() {
        use bevy::prelude::*;
        let mut app = App::new();
        app.add_message::<bevy::app::AppExit>();
        let entity = app
            .world_mut()
            .spawn(bevy::window::Window {
                resolution: (1920, 1080).into(),
                ..default()
            })
            .id();
        install_deadline(
            &mut app,
            DiagnosticBudget {
                deadline: std::time::Instant::now() + std::time::Duration::from_secs(MAX_LIFETIME),
                frame_rate: render_model::FrameRate::from_hz(MAX_FRAME_CAP).unwrap(),
            },
        );
        app.update();
        assert_eq!(
            app.world()
                .get::<bevy::window::Window>(entity)
                .unwrap()
                .resolution
                .physical_size()
                .to_array(),
            render_model::ENHANCED_DIAGNOSTIC_MAX_VIEWPORT
        );
    }

    #[test]
    fn diagnostics_require_explicit_mode_and_nonzero_bounded_budgets() {
        let mut args = ClientArgs::default();
        assert!(!bounded_request(&args));
        args.render_mode = Some(ui::RenderMode::Enhanced);
        args.frame_cap = Some(MAX_FRAME_CAP);
        args.acceptance_seconds = Some(MAX_LIFETIME);
        assert!(bounded_request(&args));
        for cap in [None, Some(0), Some(MAX_FRAME_CAP + 1)] {
            args.frame_cap = cap;
            assert!(!bounded_request(&args));
        }
        args.frame_cap = Some(1);
        for seconds in [None, Some(0), Some(MAX_LIFETIME + 1)] {
            args.acceptance_seconds = seconds;
            assert!(!bounded_request(&args));
        }
    }
}

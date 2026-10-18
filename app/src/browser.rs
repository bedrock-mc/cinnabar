//! Browser application composition over the client's shared renderer and world owners.

use assets::carriers::startup::delivery::StartupCarriers;
use bevy::{
    anti_alias::AntiAliasPlugin,
    prelude::{App, DefaultPlugins, PluginGroup, Window, default},
    render::{RenderPlugin, settings::RenderCreation},
    window::WindowPlugin,
};

thread_local! {
    static STARTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

mod frame;
pub(crate) mod jobs;
mod session;
mod startup;
pub use session::{Event, Host};

/// Starts the renderer on an existing canvas, retaining the verified world inputs.
pub fn run(
    startup: StartupCarriers,
    canvas: String,
    identity: protocol::ClientRandomId,
    input: Box<dyn FnMut() -> semantic_input::DeviceFrame>,
    audio: Option<client_audio::audio::SoundBank>,
    output: client_audio::named_audio::AudioDevice,
) -> anyhow::Result<Host> {
    anyhow::ensure!(
        !STARTED.with(std::cell::Cell::get),
        "The client is already started; reload the page to restart it"
    );
    let mut settings = bevy::render::settings::WgpuSettings::default();
    render::device_settings::configure(&mut settings);
    let mut app = App::new();
    let (host, commands) = startup::install(&mut app, startup, identity, audio, output)?;
    STARTED.with(|started| started.set(true));
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    canvas: Some(canvas),
                    fit_canvas_to_parent: true,
                    prevent_default_event_handling: false,
                    ..default()
                }),
                ..default()
            })
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(Box::new(settings)),
                ..default()
            })
            .disable::<AntiAliasPlugin>(),
    );
    app.insert_non_send(commands)
        .insert_non_send(crate::browser_input::InputSource(input))
        .add_plugins((
            render::ActorRenderPlugin,
            render::AtmospherePlugin,
            render::ChunkRenderPlugin::with_budget(
                client_runtime::publication::PublicationController::new(
                    render_api::PublicationServiceConfig::DEFAULT,
                )
                .budget(),
            ),
            crate::camera::FlyCameraPlugin::default(),
            render::UiRenderPlugin,
            render::ViewmodelRenderPlugin,
            render::HandRigRenderPlugin,
            render::DroppedItemRenderPlugin,
            render::ScreenOverlayRenderPlugin,
            render::AimAssistHighlightPlugin,
            render::ParticleRenderPlugin,
            render::BlockEntityRenderPlugin,
            render::EntityShadowRenderPlugin,
        ))
        .add_plugins((
            render::DepthSmaaPlugin,
            render::motion_blur::CameraMotionBlurPlugin,
            render::startup_presentation::StartupPresentationPlugin,
        ));
    frame::configure(&mut app);
    let exit = app.run();
    anyhow::ensure!(!exit.is_error(), "browser client stopped with an error");
    Ok(host)
}

/// Formats a protocol-validated transfer through the shared session address owner.
pub fn transfer_address(host: &str, port: u16) -> String {
    client_runtime::session::format_transfer_address(host, port)
}

/// Browser-owned drivers for the world's and UI's existing bounded work queues.
#[derive(Default)]
pub struct Jobs {
    world: chunk_pipeline::jobs::WorldJobDriver,
    images: ui_engine::atlas::jobs::ImageJobDriver,
    ui: client_ui::browser_jobs::Driver,
    next: usize,
}

impl Jobs {
    /// Runs at most one finite job after the host yields; returns whether work was performed.
    pub fn run_one(&mut self, now: std::time::Duration) -> bool {
        let now = simulation_clock::Moment::from_elapsed(now);
        self.world.observe(now);
        let drivers: &[fn(&mut Self, simulation_clock::Moment) -> bool] = &[
            |jobs, now| jobs.world.run_one(now),
            |jobs, _| jobs.images.run_one(),
            |_, _| client_world::browser_jobs::run_one(),
            |jobs, _| jobs.ui.run_one(),
            |_, _| render::browser_jobs::run_one(),
            |_, _| client_runtime::browser_jobs::run_one(),
            |_, _| jobs::run_one(),
        ];
        for _ in drivers {
            let driver = drivers[self.next];
            self.next = (self.next + 1) % drivers.len();
            if driver(self, now) {
                return true;
            }
        }
        false
    }
}

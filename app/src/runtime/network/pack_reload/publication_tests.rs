use super::*;
use bevy::prelude::*;
use render_scene::{ActorArtworkResource, WeatherTextureAssets};
use std::io::Cursor;

#[derive(Resource)]
struct ExpectedVisuals {
    sky_pixels: Vec<u8>,
    artwork: render_model::ActorArtworkPages,
    observations: usize,
}

/// Checks the resources consumed by world, UI and render publication in the current frame.
fn observe_visuals(
    weather: Res<WeatherTextureAssets>,
    artwork: Res<ActorArtworkResource>,
    reload: Res<PackReload>,
    mut expected: ResMut<ExpectedVisuals>,
) {
    assert_eq!(reload.completed_revision(), 1);
    assert_ne!(
        weather.identity(),
        WeatherTextureAssets::default().identity()
    );
    assert_eq!(
        weather.end_sky().unwrap().rgba8.as_ref(),
        expected.sky_pixels
    );
    assert_eq!(artwork.identity(), expected.artwork.identity());
    assert_eq!(artwork.pages(), expected.artwork.pages());
    expected.observations += 1;
}

#[test]
fn shared_frame_publishes_server_visuals_before_consumers() {
    let _sounds = client_audio::audio::SERVER_SOUNDS_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut png = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(1, 1, image::Rgba([37, 59, 83, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let sky_path = format!("{}.png", assets::weather_textures::END_SKY_TEXTURE_PATH);
    let stack = crate::runtime::network::pack_reload_tests::stack(&[(&sky_path, png.get_ref())]);
    let view = resource_pack::LayeredPackView::new(stack.clone());
    let weather = WeatherTextureAssets::default();
    let environment = prepare_environment(
        &view,
        &EnvironmentBase::new(Default::default(), None, weather.clone()),
        true,
        false,
    );
    let mut artwork = render_model::ActorArtworkPages::default();
    artwork.identity = [7; 32];
    artwork.pages = Arc::from([render_model::ActorTexturePage {
        width: 1,
        height: 1,
        layers: 1,
        rgba8: Arc::from([11, 23, 47, 255]),
        color_mask: false,
        multitexture: false,
    }]);
    // A texture-only replacement has artwork without new entity definitions.
    let application = PackApplication {
        admission: PackAdmission::Validated(stack),
        entity_artwork: Some(Arc::new(artwork.clone())),
        ..Default::default()
    };
    let assets = Arc::new(assets::RuntimeAssets::diagnostic());
    let mut app = App::new();
    crate::game_frame::configure_client_frame_schedule(&mut app);
    crate::game_frame::configure_pack_publication(&mut app);
    app.insert_resource(ClientWorld::new(assets.clone()))
        .insert_resource(UiRuntimeResource(client_ui::ui_runtime::UiRuntime::new(0)))
        .insert_resource(weather)
        .init_resource::<ActorArtworkResource>()
        .insert_resource(ExpectedVisuals {
            sky_pixels: vec![37, 59, 83, 255],
            artwork,
            observations: 0,
        })
        .add_systems(
            Update,
            (
                observe_visuals.in_set(client_state::ClientFrameSet::WorldPublication),
                observe_visuals.in_set(client_state::ClientFrameSet::UiPublication),
                observe_visuals.in_set(render_scene::ChunkRenderApplySet),
            ),
        );
    let mut reload = app.world_mut().resource_mut::<PackReload>();
    reload.begin_session(1, &application);
    // Deliver a completed worker result so this test exercises only frame publication.
    reload.ready = Some(Prepared {
        revision: reload.revision,
        generation: reload.generation,
        application,
        assets,
        environment: Some(environment),
        elapsed: Duration::ZERO,
    });
    app.update();
    assert_eq!(app.world().resource::<ExpectedVisuals>().observations, 3);
}

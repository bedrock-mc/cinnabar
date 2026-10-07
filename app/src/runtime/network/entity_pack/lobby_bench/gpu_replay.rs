//! Native GPU replay of captured lobby actors; no socket or live server is used.
use super::*;
use bevy::{
    anti_alias::{AntiAliasPlugin, fxaa::Fxaa},
    asset::AssetPlugin,
    camera::{Camera3dDepthTextureUsage, CameraPlugin, RenderTarget},
    core_pipeline::{CorePipelinePlugin, tonemapping::Tonemapping},
    mesh::MeshPlugin,
    post_process::{PostProcessPlugin, bloom::Bloom},
    prelude::*,
    render::{RenderApp, RenderPlugin, render_resource::*, renderer::RenderDevice, view::Hdr},
    window::WindowPlugin,
};

const VIEWPORT: [u32; 2] = [640, 360];

/// Uses the production plugins and camera attachments on an offscreen target.
fn gpu_app(camera: Transform) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(WindowPlugin {
            primary_window: None,
            ..default()
        })
        .add_plugins(AssetPlugin::default())
        .add_plugins(RenderPlugin {
            synchronous_pipeline_compilation: true,
            ..default()
        })
        .add_plugins((
            ImagePlugin::default(),
            MeshPlugin,
            CameraPlugin,
            CorePipelinePlugin,
            PostProcessPlugin,
            AntiAliasPlugin,
        ))
        .add_plugins((
            render::ChunkRenderPlugin::new(1),
            render::AtmospherePlugin,
            render::ActorRenderPlugin,
            render::UiRenderPlugin,
            render::HandRigRenderPlugin,
            render::ViewmodelRenderPlugin,
            render::ParticleRenderPlugin,
            render::DroppedItemRenderPlugin,
            render::ScreenOverlayRenderPlugin,
            render::EnhancedRenderPlugin,
        ));
    let image = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            VIEWPORT[0],
            VIEWPORT[1],
            TextureFormat::Rgba8Unorm,
            Some(TextureFormat::Rgba8UnormSrgb),
        ));
    app.world_mut().spawn((
        Camera3d {
            depth_texture_usages: Camera3dDepthTextureUsage::from(
                TextureUsages::RENDER_ATTACHMENT
                    | TextureUsages::TEXTURE_BINDING
                    | TextureUsages::COPY_SRC,
            ),
            ..default()
        },
        Camera::default(),
        RenderTarget::Image(image.into()),
        Msaa::Off,
        Fxaa::default(),
        Hdr,
        Tonemapping::None,
        Bloom::default(),
        render::EnhancedRendering::default(),
        camera,
    ));
    app.finish();
    app.cleanup();
    app
}

/// Loads the real font, HUD and JSON-UI carriers used by offline screen snapshots.
fn ui_presentation() -> client_ui::ui_runtime::presentation::UiPresentationRuntime {
    use crate::ui_runtime::presentation::forms::pack_harness;
    use client_ui::ui_runtime::presentation::UiPresentationRuntime;
    let world_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(crate::asset_startup::DEFAULT_ASSET_PATH);
    let hud = crate::asset_startup::require_hud_assets(&world_path)
        .unwrap()
        .into_runtime();
    let mut presentation = UiPresentationRuntime::with_hud(pack_harness::font(), hud).unwrap();
    presentation
        .enable_json_ui(pack_harness::carrier().expect("installed JSON-UI carrier"))
        .unwrap();
    presentation.hud_frame_mut().first_person = true;
    presentation
}

/// Publishes the production menu/HUD composite into the offscreen graph.
fn publish_ui(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    app: &mut App,
    presentation: &mut client_ui::ui_runtime::presentation::UiPresentationRuntime,
    runtime: &client_ui::ui_runtime::UiRuntime,
    millis: u64,
) {
    let input = presentation
        .build(
            player_runtime,
            runtime,
            millis,
            VIEWPORT,
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let stats = app
        .world()
        .resource::<render::UiRenderStatsResource>()
        .clone();
    app.world_mut()
        .resource_mut::<render::UiRenderSceneResource>()
        .publish(input, &stats)
        .unwrap();
}

#[test]
#[ignore = "requires the local captured lobby and its resource pack"]
fn enhanced_lobby_replay_on_native_gpu() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let capture = std::env::var_os("CINNABAR_LOBBY_CAPTURE").expect("offline capture path");
    let pack = std::env::var_os("CINNABAR_RENDER_PACK").expect("offline pack path");
    let capture = read_capture(Path::new(&capture));
    let (mut world, rest, mut replay) = build_world(&capture, Some(Path::new(&pack)), false);
    let camera = *world
        .query_filtered::<&Transform, With<crate::camera::FlyCamera>>()
        .single(&world)
        .unwrap();
    let mut app = gpu_app(camera);
    let mut presentation = ui_presentation();
    let runtime = client_ui::ui_runtime::UiRuntime::new(1);
    presentation.set_menu_view(Some(
        crate::menu::MenuRuntime::new(true, 2, "Steve".into()).view(),
    ));
    for millis in 0..3 {
        publish_ui(
            &player_runtime,
            &mut app,
            &mut presentation,
            &runtime,
            millis,
        );
        app.update();
    }
    presentation.set_menu_view(None);
    let font = crate::ui_runtime::presentation::forms::pack_harness::font();
    let mut layouts = ui::TextLayoutCache::new(256, 1 << 20);
    let mut atlas = client_ui::ui_runtime::presentation::nametag_atlas::NametagAtlas::default();
    let mut clock = Instant::now();
    let mut next = 0;
    for frame in 0..120 {
        let due = rest.len() * (frame + 1) / 120;
        {
            let mut client_world = world.resource_mut::<crate::runtime::world::ClientWorld>();
            let stream = client_world.stream.as_mut().unwrap();
            while next < due {
                let (id, body) = &rest[next];
                replay.apply(stream, *id, body);
                next += 1;
            }
            drain(stream, replay.local_position);
        }
        clock += FRAME;
        world
            .resource_mut::<Time<Real>>()
            .update_with_instant(clock);
        prepare_offline_actor_frame(&mut world);
        world.run_system_cached(publish_actor_render_frame).unwrap();
        app.world_mut()
            .insert_resource(world.resource::<ActorRenderFrame>().clone());
        app.world_mut()
            .insert_resource(world.resource::<render::HandRigScene>().clone());
        publish_ui(
            &player_runtime,
            &mut app,
            &mut presentation,
            &runtime,
            frame as u64 * 16,
        );
        {
            use client_ui::ui_runtime::presentation::{nametag_atlas, nametags};
            let client = world.resource::<crate::runtime::world::ClientWorld>();
            let stream = client.stream.as_ref().unwrap();
            let anchors: Vec<_> = stream
                .authority()
                .actor_rigs()
                .filter_map(|rig| {
                    let actor = stream.authority().actor(rig.actor.runtime_id)?;
                    nametags::extract_nametag(
                        actor,
                        camera.translation,
                        None,
                        stream.authority().actor_name_tag(actor.unique_id)?,
                        &ui::ScoreboardStore::default(),
                        1.0,
                    )
                })
                .collect();
            app.world_mut()
                .insert_resource(render::NametagSceneResource(nametags::build_nametag_scene(
                    &anchors,
                    &font,
                    &mut layouts,
                    &mut atlas,
                    &|page| nametag_atlas::font_page(&font, page),
                )));
        }
        app.update();
        app.sub_app(RenderApp)
            .world()
            .resource::<RenderDevice>()
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
    }
    let cache = app.sub_app(RenderApp).world().resource::<PipelineCache>();
    for pipeline in cache.pipelines() {
        if let CachedPipelineState::Err(error) = &pipeline.state {
            panic!("lobby GPU pipeline failed: {error}");
        }
    }
    assert!(
        !app.world()
            .resource::<render::ActorPresentationGate>()
            .drain()
            .is_empty()
    );
}

//! Offline menu frames through the native GPU and production UI pass.
use super::{pack_harness, play_flow_snapshots};
use crate::menu::MenuScreen;
use bevy::{
    asset::AssetPlugin,
    camera::{CameraPlugin, RenderTarget},
    core_pipeline::{CorePipelinePlugin, tonemapping::Tonemapping},
    mesh::MeshPlugin,
    prelude::*,
    render::{
        RenderApp, RenderPlugin,
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::*,
        renderer::RenderDevice,
    },
    window::WindowPlugin,
};
use client_ui::ui_runtime::presentation::forms::panorama;
use std::{sync::Arc, time::Instant};

const SIZE: [u32; 2] = [1280, 720];
#[derive(Resource, Default)]
struct Captured(Vec<u8>);

/// Builds an offscreen camera with the same retained UI and panorama passes as startup.
fn app() -> App {
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
        ))
        .add_plugins((render::UiRenderPlugin, render::PanoramaRenderPlugin));
    let mut image = Image::new_target_texture(
        SIZE[0],
        SIZE[1],
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    );
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let image = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    app.world_mut().spawn((
        Camera3d::default(),
        Camera::default(),
        RenderTarget::Image(image.clone().into()),
        Msaa::Off,
        Tonemapping::None,
    ));
    app.init_resource::<Captured>();
    app.world_mut().spawn(Readback::texture(image)).observe(
        |event: On<ReadbackComplete>, mut capture: ResMut<Captured>| {
            capture.0.clone_from(&event.data);
        },
    );
    let mut scene = app.world_mut().resource_mut::<render::PanoramaScene>();
    scene.set_game_visible(false);
    scene.set_faces(panorama::built_in_faces().map(Arc::new));
    scene.show(Some(panorama::launcher_view(
        0.0,
        SIZE[0] as f32 / SIZE[1] as f32,
        [0.0; 4],
    )));
    app.finish();
    app.cleanup();
    app
}

#[test]
#[ignore = "offline native GPU menu timings and snapshots"]
fn menu_frames_on_native_gpu() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let mut presentation = pack_harness::startup_presentation().expect("installed carriers");
    let dir = pack_harness::scratch_dir("gpu-menu");
    let mut view = play_flow_snapshots::fixture_view(&dir);
    view.feeds.home.inbox = [
        ("A new adventure awaits", "2026-10-03T10:00:00Z", true),
        ("Explore the latest update", "2026-08-01T10:00:00Z", false),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (title, date, unread))| crate::menu::InboxItem {
        instance_id: format!("offline-{i}"),
        header: title.into(),
        received: date.into(),
        source: "Minecraft".into(),
        category: "News".into(),
        unread,
        ..Default::default()
    })
    .collect();
    let mut app = app();
    let mut runtime = pack_harness::menu_runtime();
    runtime.publish_inventory_authority(&mut player_runtime, protocol::InventoryAuthority::Server);
    runtime
        .publish_local_runtime_id(&mut player_runtime, 1, 42)
        .unwrap();
    let stats = app.world().resource::<render::UiRenderStats>().clone();
    let skin = crate::player_skin::LocalPlayerSkin::generated_default("Test");
    let skin_pixels = image::open(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../.local")
            .join(crate::install_layout::vanilla_pack_relative())
            .join("textures/entity/steve.png"),
    )
    .ok()
    .map(|image| image.to_rgba8().into_raw());
    for (name, screen) in [
        ("home", MenuScreen::Home),
        ("inbox", MenuScreen::Inbox),
        ("pause", MenuScreen::Pause),
        ("paper-doll", MenuScreen::Home),
        ("inventory", MenuScreen::Home),
        ("play", MenuScreen::Play),
        ("servers", MenuScreen::Servers),
        ("edit", MenuScreen::AddServer),
        ("settings", MenuScreen::Settings),
        ("loading", MenuScreen::Home),
    ] {
        if name == "inventory" {
            runtime.toggle_inventory(&mut player_runtime);
            assert!(runtime.inventory_open());
        }
        if name == "play" && runtime.inventory_open() {
            runtime.toggle_inventory(&mut player_runtime);
        }
        view.screen = screen;
        view.editing = (screen == MenuScreen::AddServer).then_some(0);
        if name == "loading" {
            presentation.set_menu_view(None);
            presentation.set_loading_stage(Some(super::LoadingStage::BuildingTerrain));
            runtime.set_server_ui(pack_harness::env_pack().map(Arc::new));
            runtime.set_session_glyphs(pack_harness::env_glyphs());
            app.world_mut()
                .resource_mut::<render::PanoramaScene>()
                .show(None);
        }
        let mut samples = Vec::new();
        let mut phases = [std::time::Duration::ZERO; 4];
        let mut geometry = (0, 0);
        for frame in 0..40 {
            let started = Instant::now();
            presentation.sync_player_preview(
                (name != "loading")
                    .then_some(skin_pixels.as_deref().unwrap_or(skin.rgba8.as_ref())),
                Default::default(),
                name != "loading",
                false,
                frame as f64 * 0.016,
            );
            view.profile_icon = presentation.player_preview_icon();
            presentation.hud_frame_mut().player_preview = presentation.player_preview_icon();
            if name != "loading" {
                presentation.sync_menu_artwork(super::super::menu_artwork::view_paths(&view));
                presentation.set_menu_view(Some(view.clone()));
            }
            let preview_done = Instant::now();
            if matches!(name, "paper-doll" | "inventory") {
                presentation.set_menu_view(None);
                player_runtime
                    .facts
                    .publish_player_game_mode(protocol::PlayerGameMode::Survival);
                presentation.hud_frame_mut().player_preview = presentation.player_preview_icon();
                presentation.hud_frame_mut().paper_doll_visible = true;
            }
            let input = presentation
                .build(
                    &player_runtime,
                    &runtime,
                    frame * 16,
                    SIZE,
                    ui::DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
            let paint_done = Instant::now();
            geometry = (input.vertices.len(), input.batches.len());
            app.world_mut()
                .resource_mut::<render::UiRenderScene>()
                .publish(input, &stats)
                .unwrap();
            let published = Instant::now();
            app.update();
            app.sub_app(RenderApp)
                .world()
                .resource::<RenderDevice>()
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            if frame >= 10 {
                samples.push(started.elapsed());
                for (total, elapsed) in phases.iter_mut().zip([
                    preview_done - started,
                    paint_done - preview_done,
                    published - paint_done,
                    published.elapsed(),
                ]) {
                    *total += elapsed;
                }
            }
        }
        samples.sort();
        eprintln!(
            "native-menu {name} median={:.3}ms p95={:.3}ms",
            samples[15].as_secs_f64() * 1000.0,
            samples[28].as_secs_f64() * 1000.0
        );
        eprintln!(
            "native-menu {name} mean preview/build/publish/GPU={:?}ms vertices={} batches={}",
            phases.map(|time| time.as_secs_f64() * 1000.0 / 30.0),
            geometry.0,
            geometry.1
        );
        if let Some(output) = std::env::var_os("CINNABAR_FORM_SNAPSHOT_DIR") {
            let output = std::path::PathBuf::from(output);
            std::fs::create_dir_all(&output).unwrap();
            image::RgbaImage::from_raw(
                SIZE[0],
                SIZE[1],
                app.world().resource::<Captured>().0.clone(),
            )
            .expect("GPU readback")
            .save(output.join(format!("native-{name}.png")))
            .unwrap();
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// Compares submitted GPU pages with the CPU publication while pack/art owners change.
#[test]
#[ignore = "offline native GPU Zeqa page ordering"]
fn zeqa_late_pages_match_the_published_frame_on_gpu() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let pack = pack_harness::env_pack().expect("CINNABAR_FORM_PACK_DIR");
    let mut presentation = pack_harness::startup_presentation().expect("installed carriers");
    let mut runtime = pack_harness::menu_runtime();
    let root = pack_harness::scratch_dir("zeqa-gpu");
    let view = play_flow_snapshots::fixture_view(&root);
    let paths = super::super::menu_artwork::view_paths(&view);
    presentation.sync_menu_artwork(paths.clone());
    presentation.set_menu_view(Some(view));
    let mut app = app();
    let stats = app.world().resource::<render::UiRenderStats>().clone();
    for phase in 0..3 {
        if phase != 0 {
            presentation.set_menu_view(None);
            presentation.set_loading_stage(Some(super::LoadingStage::BuildingTerrain));
            crate::session::begin_session(&mut runtime, &mut player_runtime, phase + 1);
            runtime.set_server_ui(Some(Arc::new(super::loading_sequence_tests::lazy(
                pack.clone(),
            ))));
            runtime.set_session_glyphs(pack_harness::env_glyphs());
        }
        for frame in 0..16 {
            presentation.sync_menu_artwork(
                paths
                    .iter()
                    .take(frame % (paths.len() + 1))
                    .cloned()
                    .collect(),
            );
            presentation.sync_player_preview(None, Default::default(), phase == 0, false, 0.0);
            let input = presentation
                .build(
                    &player_runtime,
                    &runtime,
                    0,
                    SIZE,
                    ui::DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
            let expected = (phase != 0).then(|| super::snapshot::rasterize(&input));
            app.world_mut()
                .resource_mut::<render::UiRenderScene>()
                .publish(input, &stats)
                .unwrap();
            app.world_mut()
                .resource_mut::<render::PanoramaScene>()
                .show(None);
            // Readback completion is delivered to the main world on the following update.
            for _ in 0..3 {
                app.update();
                app.sub_app(RenderApp)
                    .world()
                    .resource::<RenderDevice>()
                    .poll(wgpu::PollType::wait_indefinitely())
                    .unwrap();
            }
            if let Some(expected) = expected {
                let actual = &app.world().resource::<Captured>().0;
                for y in (0..SIZE[1]).step_by(19) {
                    for x in (0..SIZE[0]).step_by(23) {
                        let offset = ((y * SIZE[0] + x) * 4) as usize;
                        for channel in 0..3 {
                            assert!(
                                actual[offset + channel]
                                    .abs_diff(expected.get_pixel(x, y)[channel])
                                    <= 2,
                                "phase {phase} frame {frame} GPU pixel {x},{y} channel {channel}: {} != {}",
                                actual[offset + channel],
                                expected.get_pixel(x, y)[channel]
                            );
                        }
                    }
                }
            }
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

/// Renders Profile fixtures on the native GPU without account auth or a server.
#[test]
#[ignore = "offline native GPU Profile frames"]
fn profile_frames_on_native_gpu() {
    let Some(mut presentation) = pack_harness::startup_presentation() else {
        return;
    };
    let mut view = crate::menu::MenuRuntime::new(true, 2, "Steve".into()).view();
    view.screen = MenuScreen::Profile;
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);
    let mut runtime = pack_harness::menu_runtime();
    runtime.publish_inventory_authority(&mut player_runtime, protocol::InventoryAuthority::Server);
    let skin = crate::player_skin::LocalPlayerSkin::generated_default("Test");
    presentation.sync_player_preview(Some(&skin.rgba8), Default::default(), true, false, 0.0);
    view.profile_icon = presentation.player_preview_icon();
    let mut app = app();
    let stats = app.world().resource::<render::UiRenderStats>().clone();
    for state in [
        "signed-out",
        "loading",
        "overview",
        "stats",
        "empty",
        "error",
    ] {
        view.auth_state = if state == "signed-out" {
            crate::menu::auth::AuthState::SignedOut
        } else {
            crate::menu::auth::AuthState::Authenticated
        };
        view.profile_tab = if matches!(state, "stats" | "empty") {
            launcher::menu::ProfileTab::Stats
        } else {
            launcher::menu::ProfileTab::Overview
        };
        let profile = &mut view.feeds.profile;
        profile.loaded = state != "loading";
        profile.avatar_loaded = true;
        profile.featured_screenshot_loaded = true;
        profile.featured_screenshot_error = false;
        profile.avatar_error = true;
        profile.achievements_loaded = true;
        profile.achievements_error = false;
        profile.achievements = Some(protocol::launcher_control::ProfileAchievements {
            unlocked: 2,
            total: 10,
            current_gamerscore: Some(20),
            max_gamerscore: Some(100),
            entries: Vec::new(),
        });
        profile.unavailable = state == "error";
        profile.gamertag = "Steve".into();
        profile.presence = "Minecraft".into();
        profile.friends = Some(17);
        profile.followers = Some(24);
        profile.statistics_loaded = true;
        profile.statistics_error = false;
        profile.statistics = Some(protocol::launcher_control::ProfileStatistics {
            minutes_played: (state != "empty").then(|| "120.5".into()),
            blocks_broken: (state != "empty").then(|| "12345".into()),
            mobs_defeated: (state != "empty").then(|| "0".into()),
            distance_travelled: (state != "empty").then(|| "1234567.89".into()),
        });
        for frame in 0..8 {
            presentation.set_menu_view(Some(view.clone()));
            let input = presentation
                .build(
                    &player_runtime,
                    &runtime,
                    frame * 16,
                    SIZE,
                    ui::DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
            app.world_mut()
                .resource_mut::<render::UiRenderScene>()
                .publish(input, &stats)
                .unwrap();
            app.update();
            app.sub_app(RenderApp)
                .world()
                .resource::<RenderDevice>()
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
        }
        if let Some(output) = std::env::var_os("CINNABAR_FORM_SNAPSHOT_DIR") {
            let output = std::path::PathBuf::from(output);
            std::fs::create_dir_all(&output).unwrap();
            image::RgbaImage::from_raw(
                SIZE[0],
                SIZE[1],
                app.world().resource::<Captured>().0.clone(),
            )
            .expect("GPU readback")
            .save(output.join(format!("native-profile-{state}.png")))
            .unwrap();
        }
    }
}

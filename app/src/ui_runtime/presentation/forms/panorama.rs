use bevy::{
    prelude::{Local, Query, Res, ResMut, With},
    window::{PrimaryWindow, Window},
};
use client_shell::menu::MenuRuntime;
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
use launcher::menu::settings_options::SettingId;
use render_model::panorama::{launcher_faces, launcher_view, overlay_tint};
use render_scene::PanoramaScene;
use std::{sync::Arc, time::Instant};
/// Uploads the faces on first sight of the carrier and shows the panorama
/// behind launcher screens; in-world Settings retains the game beneath it.
/// While the world shows without the panorama the faces are released, so their
/// texture leaves the GPU, and decoded again when a launcher screen returns.
#[allow(
    clippy::too_many_arguments,
    reason = "Bevy system parameters: the screen state the panorama follows"
)]
pub(crate) fn drive_menu_panorama(
    player_runtime: bevy::prelude::Res<client_state::PlayerRuntime>,
    presentation: Res<UiPresentationResource>,
    runtime: Option<Res<UiRuntimeResource>>,
    menu: Option<Res<MenuRuntime>>,
    scene: Option<ResMut<PanoramaScene>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut state: Local<Option<(Instant, f32, [f32; 4])>>,
    mut released: Local<bool>,
) {
    let Some(mut scene) = scene else {
        return;
    };
    // The renderer keeps the faces on the GPU; their pixels are only needed until then.
    scene.release_uploaded_faces();
    let renders_game = client_shell::screen_policy::renders_game(
        &player_runtime,
        runtime.as_deref().map(|runtime| &runtime.0),
        menu.as_deref(),
        Some(&presentation),
    );
    scene.set_game_visible(renders_game);
    let Some(assets) = presentation.ui_assets() else {
        scene.show(None);
        return;
    };
    if state.is_none() {
        *state = Some((Instant::now(), 0.0, overlay_tint(assets)));
        scene.set_faces(launcher_faces(assets).map(Arc::new));
    }
    let shown = presentation
        .loading_stage()
        .is_some_and(ui::LoadingStage::is_world_entry)
        || menu.as_deref().is_some_and(MenuRuntime::uses_panorama);
    if *released && shown {
        scene.set_faces(launcher_faces(assets).map(Arc::new));
        *released = false;
    } else if !*released && !shown && renders_game && scene.has_faces() {
        scene.set_faces(None);
        *released = true;
    }
    let aspect = windows
        .iter()
        .next()
        .map(|window| window.width() / window.height().max(1.0))
        .unwrap_or(16.0 / 9.0);
    let Some((last, seconds, tint)) = state.as_mut() else {
        return;
    };
    let now = Instant::now();
    let speed = menu.as_ref().map_or(1.0, |menu| {
        menu.settings_snapshot().0.value(SettingId::PanoramaSpeed) as f32 / 100.0
    });
    *seconds += now.duration_since(*last).as_secs_f32() * speed;
    *last = now;
    let has_faces = scene.has_faces();
    scene.show((shown && has_faces).then(|| launcher_view(*seconds, aspect, *tint)));
}

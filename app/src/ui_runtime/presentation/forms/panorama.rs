use crate::menu::MenuRuntime;
use bevy::{
    prelude::{Local, Query, Res, ResMut, With},
    window::{PrimaryWindow, Window},
};
use client_ui::ui_runtime::presentation::forms::panorama::{
    launcher_faces, launcher_view, overlay_tint,
};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use render::PanoramaScene;
use std::{sync::Arc, time::Instant};
/// Uploads the faces on first sight of the carrier and shows the panorama
/// behind launcher screens (never behind the in-game pause or death screens).
pub(crate) fn drive_menu_panorama(
    player_runtime: bevy::prelude::Res<crate::player_runtime::PlayerRuntime>,
    presentation: Res<UiPresentationRuntime>,
    runtime: Option<Res<UiRuntime>>,
    menu: Option<Res<MenuRuntime>>,
    scene: Option<ResMut<PanoramaScene>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut state: Local<Option<(Instant, f32, [f32; 4])>>,
) {
    let Some(mut scene) = scene else {
        return;
    };
    scene.set_game_visible(crate::screen_policy::renders_game(
        &player_runtime,
        runtime.as_deref(),
        menu.as_deref(),
        Some(&presentation),
    ));
    let Some(assets) = presentation.ui_assets() else {
        scene.show(None);
        return;
    };
    if state.is_none() {
        *state = Some((Instant::now(), 0.0, overlay_tint(assets)));
        scene.set_faces(launcher_faces(assets).map(Arc::new));
    }
    let shown = menu.as_deref().is_some_and(MenuRuntime::uses_panorama);
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
        menu.settings_snapshot().0.value("panorama_speed") as f32 / 100.0
    });
    *seconds += now.duration_since(*last).as_secs_f32() * speed;
    *last = now;
    let has_faces = scene.has_faces();
    scene.show((shown && has_faces).then(|| launcher_view(*seconds, aspect, *tint)));
}

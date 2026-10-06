//! Composes committed debug drawing with the session, player pose and active font.

use crate::{app::ClientFrameSet, local_player::LocalViewPose, runtime::world::ClientWorld};
use bevy::prelude::*;
use client_presentation::{
    actor_publication::ActorFramePartialTick, primitive_shapes::PrimitiveShapePublisher,
};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use render::{PrimitiveShapesRenderPlugin, PrimitiveShapesScene};

/// Runs after packet commitment and actor interpolation, before render extraction.
pub(crate) fn configure(app: &mut App) {
    app.add_plugins(PrimitiveShapesRenderPlugin)
        .add_systems(Update, publish.in_set(ClientFrameSet::WorldPublication));
}

/// Shares retained state across render extraction without cloning or walking shape records.
fn publish(
    mut world: ResMut<ClientWorld>,
    mut scene: ResMut<PrimitiveShapesScene>,
    time: Res<Time>,
    partial: Res<ActorFramePartialTick>,
    view: Res<LocalViewPose>,
    mut presentation: ResMut<UiPresentationRuntime>,
    runtime: Res<UiRuntime>,
    mut publisher: Local<PrimitiveShapePublisher>,
) {
    publisher.publish(
        world.stream.as_mut(),
        &mut scene,
        time.elapsed_secs(),
        partial.0,
        Some(view.feet_translation().to_array()),
    );
    let mut store = scene.store.lock().expect("primitive store lock poisoned");
    presentation.prepare_primitive_text(&mut store, &runtime);
}

/// Exposes retained drawing counters only when a developer-control snapshot is requested.
#[cfg(feature = "developer-control")]
pub(crate) fn snapshot(world: &World) -> serde_json::Value {
    let Some(scene) = world.get_resource::<PrimitiveShapesScene>() else {
        return serde_json::Value::Null;
    };
    let store = scene.store.lock().expect("primitive store lock poisoned");
    serde_json::json!({
        "shapes": store.len(),
        "skipped_entries": store.skipped_entries,
        "instance_rebuilds": store.instance_rebuilds,
        "mesh_batches": store.batches().iter().filter(|batch| !batch.instances.is_empty()).count(),
        "pending_uploads": store.has_changes(),
        "dimension": scene.dimension,
        "render_distance": scene.render_distance,
    })
}

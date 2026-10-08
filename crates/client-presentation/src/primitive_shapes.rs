//! Publishes packet deltas and compact actor positions to the retained debug renderer.

use chunk_pipeline::WorldStream;
use render::PrimitiveShapesScene;
use render_model::primitive_shapes::PrimitiveShapeStore;
use std::sync::{Arc, Mutex};

/// Session identity prevents a new connection from inheriting the previous server's shapes.
#[derive(Default)]
pub struct PrimitiveShapePublisher {
    session: Option<u64>,
}

impl PrimitiveShapePublisher {
    /// Applies committed packets once and samples each attached actor once per rendered frame.
    pub fn publish(
        &mut self,
        stream: Option<&mut WorldStream>,
        scene: &mut PrimitiveShapesScene,
        now: f32,
        partial_tick: f32,
        local_feet: Option<[f32; 3]>,
    ) {
        let session = stream
            .as_ref()
            .map(|stream| stream.authority().actor_session_id());
        if self.session != session {
            scene.store = Arc::new(Mutex::new(PrimitiveShapeStore::default()));
            self.session = session;
        }
        scene.clock = now;
        let Some(stream) = stream else {
            return;
        };
        scene.dimension = stream.current_dimension();
        scene.render_distance =
            render::adjusted_player_render_distance_blocks(stream.render_distance_blocks())
                .unwrap_or(0.0);
        let mut store = scene.store.lock().expect("primitive store lock poisoned");
        while let Some(event) = stream.pop_primitive_shapes() {
            if event.skipped_entries != 0 {
                bevy::log::warn!(
                    skipped = event.skipped_entries,
                    "ignored invalid primitive shape entries"
                );
            }
            store.apply(event);
        }
        store.update_actors(|id| {
            if id == stream.authority().local_player_unique_id() {
                return local_feet;
            }
            stream
                .authority()
                .actor_by_unique_id(id)?
                .interpolated_position(partial_tick)
        });
    }
}

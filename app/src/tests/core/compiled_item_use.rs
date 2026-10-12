//! Original projection of an unsupported item-use admission.

use render_model::{UiRenderBatch, UiRenderInput, UiRenderTextureArray, UiRenderVertex, UiScissor};
use std::sync::Arc;

/// Admits an independent unknown registry item and its legacy resource duration.
fn stream() -> chunk_pipeline::WorldStream {
    let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    assert!(stream.seed_item_registry(protocol::ItemRegistryEvent {
        entries: Arc::from([protocol::ItemRegistryEntry {
            components: None,
            identifier: "minecraft:fixture_unadmitted".into(),
            network_id: 15000,
            component_based: false,
            version: protocol::ItemRegistryVersion::Legacy,
            component_digest: [0; 32],
            negotiated_max_stack_size: None,
            canonical_empty_component_data: true,
            item_tags: Arc::from([]),
        }]),
    }));
    stream.set_item_use_durations(Arc::new(std::collections::BTreeMap::from([(
        Box::from("minecraft:fixture_unadmitted"),
        32,
    )])));
    stream
}

/// Adds one original diagnostic rectangle to the offline render input.
fn rectangle(
    vertices: &mut Vec<UiRenderVertex>,
    indices: &mut Vec<u32>,
    bounds: [f32; 4],
    color: [u8; 4],
) {
    let first = vertices.len() as u32;
    for position in [
        [bounds[0], bounds[1]],
        [bounds[2], bounds[1]],
        [bounds[2], bounds[3]],
        [bounds[0], bounds[3]],
    ] {
        vertices.push(UiRenderVertex {
            position,
            clip_z: 0.0,
            clip_w: 1.0,
            uv: [0.0; 2],
            color,
            style_flags: 0,
            alpha_cutoff: -1.0,
            model_light: 1.0,
            overlay_color: [0.0; 4],
        });
    }
    indices.extend([first, first + 1, first + 2, first, first + 2, first + 3]);
}

/// Projects the actual admitted movement factor through the offline UI rasterizer.
fn snapshot(movement: f64) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    rectangle(
        &mut vertices,
        &mut indices,
        [0.0, 0.0, 128.0, 128.0],
        [22, 27, 34, 255],
    );
    rectangle(
        &mut vertices,
        &mut indices,
        [28.0, 96.0, 100.0, 110.0],
        [230, 160, 45, 255],
    );
    let feet = 32.0 + movement as f32 * 64.0;
    rectangle(
        &mut vertices,
        &mut indices,
        [feet - 8.0, 56.0, feet + 8.0, 84.0],
        [80, 190, 230, 255],
    );
    let input = UiRenderInput {
        revision: 1,
        viewport_size: [128; 2],
        safe_area: [0; 4],
        batches: Arc::from([UiRenderBatch::new(
            0,
            UiScissor::new(0, 0, 128, 128),
            0,
            indices.len() as u32,
            0,
        )]),
        vertices: vertices.into(),
        indices: indices.into(),
        textures: Arc::new(
            UiRenderTextureArray::new(
                vec![render_model::UiTexturePage::owned([1; 2], Arc::from([255; 4])).unwrap()],
                1,
            )
            .unwrap(),
        ),
    };
    input.validate().unwrap();
    let image = client_ui::ui_runtime::presentation::forms::snapshot::rasterize(&input);
    assert!(image.pixels().any(|pixel| pixel.0 == [80, 190, 230, 255]));
    client_ui::ui_runtime::presentation::forms::snapshot::write(&input, "unadmitted-item-use");
}

#[test]
fn unknown_duration_does_not_admit_a_food_use_or_movement_penalty() {
    let stream = stream();
    let action = gameplay::item_use::classify(stream.authority().item_use_profile(15000), false, 0);
    let movement = match action {
        Some(gameplay::item_use::AirUse::Hold { slowdown, .. }) => slowdown,
        _ => 1.0,
    };
    snapshot(movement);
    assert_eq!(action, None);
    assert_eq!(movement, 1.0);
}

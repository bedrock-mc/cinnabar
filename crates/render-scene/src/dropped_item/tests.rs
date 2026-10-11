use render_model::{
    DroppedItemInstance, ItemMeshVertex, MAX_DROPPED_ITEM_INSTANCES, MAX_DYNAMIC_ITEM_VERTICES,
    WHITE_LAYER, dropped_item_transform,
};

use super::*;
use render_model::OPAQUE_WHITE;

#[test]
fn publish_caps_instances_and_keeps_models_until_the_revision_changes() {
    let mut scene = DroppedItemScene::default();
    let model = DroppedItemModel::Sprite(DroppedItemSprite {
        width: 1,
        height: 1,
        rgba8: Arc::from([255_u8; 4]),
    });
    let instance = DroppedItemInstance {
        model: 0,
        world_from_item: dropped_item_transform([0.0; 3], 0.0, 1.0),
        block_level: 0,
        sky_level: 15,
        overlay_rgba8: 0,
    };
    let many = vec![instance; MAX_DROPPED_ITEM_INSTANCES + 5];
    let vertex = ItemMeshVertex {
        position: [0.0; 3],
        uv: [0.0; 2],
        normal: [0.0, 1.0, 0.0],
        layer: WHITE_LAYER,
        color: OPAQUE_WHITE,
    };
    let lines = vec![vertex; MAX_DYNAMIC_ITEM_VERTICES + 2];
    scene.publish(1, Arc::from([model]), &many, &lines, f32::NAN);
    assert_eq!(scene.instance_count(), MAX_DROPPED_ITEM_INSTANCES);
    assert_eq!(scene.dynamic.len() % 3, 0);
    assert!(scene.dynamic.len() <= MAX_DYNAMIC_ITEM_VERTICES);
    assert_eq!(scene.daylight, 1.0);
    scene.publish(1, Arc::from([]), &many[..1], &[], 0.5);
    assert_eq!(scene.models.len(), 1);
    scene.publish(2, Arc::from([]), &[], &[], 0.5);
    assert!(scene.models.is_empty());
}

#[test]
fn transform_rotates_about_up_and_translates() {
    let rows = dropped_item_transform([1.0, 2.0, 3.0], std::f32::consts::FRAC_PI_2, 2.0);
    // Local +X maps to world -Z at a quarter turn.
    assert!(rows[0][0].abs() < 1e-6 && (rows[2][0] + 2.0).abs() < 1e-6);
    assert_eq!([rows[0][3], rows[1][3], rows[2][3]], [1.0, 2.0, 3.0]);
}

use render_model::DroppedItemSprite;

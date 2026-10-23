use super::*;

#[test]
fn sprite_items_keep_their_visual_when_the_stack_carries_a_placed_block() {
    let sprite = ItemVisualRoute::Compiled(ItemVisualId(7));
    for block_runtime_id in [1, i32::MIN] {
        assert_eq!(classify_retained_block(sprite, block_runtime_id), sprite);
    }
    let block = ItemVisualRoute::BlockItem(BlockVisualId(3));
    assert_eq!(classify_retained_block(block, 1), block);
    assert_eq!(
        classify_retained_block(ItemVisualRoute::Missing, i32::MIN),
        ItemVisualRoute::RetainedBlock {
            block_runtime_id: i32::MIN,
        }
    );
}

use super::*;

#[test]
fn every_stack_reference_device_and_session_input_changes_the_key() {
    let references = References {
        world: Some([1; 32]),
        readers: vec![[7; 32]],
        keys: Some([2; 32]),
        terrain: Default::default(),
        device: assets::texture::policy::TexturePolicy::for_device(1024),
        terrain_policy: assets::texture::policy::TexturePolicy::for_terrain(1024, 0),
    };
    let stack = vec![("id".into(), "version".into(), "subpack".into(), [3; 32])];
    let inputs = PackInputs::default();
    for subscriber in ["blocks", "icons"] {
        let original = key(subscriber, &stack, &inputs, 256, &references);
        for index in 0..4 {
            let mut changed = stack.clone();
            match index {
                0 => changed[0].0.push('x'),
                1 => changed[0].1.push('x'),
                2 => changed[0].2.push('x'),
                _ => changed[0].3[0] ^= 1,
            }
            assert_ne!(
                original,
                key(subscriber, &changed, &inputs, 256, &references)
            );
        }
        for index in 0..6 {
            let mut changed = references.clone();
            match index {
                0 => changed.world = Some([4; 32]),
                1 => changed.keys = Some([5; 32]),
                2 => {
                    changed.terrain.insert("key".into(), "path".into());
                }
                3 => changed.device = assets::texture::policy::TexturePolicy::for_device(512),
                5 => changed.readers[0][0] ^= 1,
                _ => {
                    changed.terrain_policy =
                        assets::texture::policy::TexturePolicy::for_terrain(1024, 1)
                }
            }
            assert_ne!(original, key(subscriber, &stack, &inputs, 256, &changed));
        }
        for index in 0..4 {
            let mut changed = inputs.clone();
            match index {
                0 => changed.hashed = true,
                1 => changed.blocks.skipped += 1,
                2 => changed.icons.push(("fixture:item".into(), "key".into())),
                _ => changed
                    .block_items
                    .push(("fixture:item".into(), "fixture:block".into())),
            }
            assert_ne!(
                original,
                key(subscriber, &stack, &changed, 256, &references)
            );
        }
        assert_ne!(original, key(subscriber, &stack, &inputs, 128, &references));
    }
}

use super::*;

#[test]
fn terrain_storage_grows_with_memory_tiers_and_preserves_device_bounds() {
    for tier in 0..=8 {
        let policy = TexturePolicy::for_terrain(2048, tier);
        assert_eq!(
            policy.storage_bytes(),
            (MIN_TERRAIN_BYTES << tier.min(MAX_TERRAIN_MEMORY_TIER)).min(MAX_TERRAIN_BYTES)
        );
        assert_eq!(policy.max_side(), 2048);
        assert_eq!(policy.terrain_bytes(4096, 1), None);
    }
    assert_eq!(
        TexturePolicy::for_terrain(MAX_TEXTURE_SIDE, 0).storage_bytes(),
        MIN_TERRAIN_BYTES
    );
    assert_eq!(
        TexturePolicy::for_device(MAX_TEXTURE_SIDE).storage_bytes(),
        MAX_SUBSCRIBER_BYTES
    );
}

#[test]
fn terrain_can_decode_a_512px_thirty_two_frame_strip_before_resizing() {
    let policy = TexturePolicy::for_terrain(MAX_TEXTURE_SIDE, 0);
    let bytes = 512 * 512 * 4 * 32;
    assert_eq!(policy.source_bytes(512, 512 * 32, true), Some(bytes));
    assert_eq!(policy.source_bytes(512 * 32, 512, true), Some(bytes));
    assert_eq!(
        TexturePolicy::for_device(MAX_TEXTURE_SIDE).source_bytes(512, 512 * 32, true),
        None
    );
    assert!(policy.decode_bytes() >= bytes as u64);
}

#[test]
fn strips_are_bounded_by_frame_dimensions_and_checked_area() {
    let policy = TexturePolicy::for_device(2048);
    assert_eq!(policy.source_bytes(64, 8384, true), Some(64 * 8384 * 4));
    assert_eq!(
        policy.source_bytes(2048, 2048, false),
        Some(MAX_IMAGE_BYTES)
    );
    assert_eq!(policy.source_bytes(64, 8384, false), None);
    assert_eq!(policy.source_bytes(64, 8385, true), None);
    assert_eq!(policy.source_bytes(2048, 4096, true), None);
    assert_eq!(policy.source_bytes(0, 64, true), None);
    assert_eq!(policy.source_bytes(u32::MAX, u32::MAX, true), None);
    assert_eq!(
        TexturePolicy::for_device(1024).source_bytes(2048, 2048, false),
        None
    );
}

#[test]
fn horizontal_strips_obey_frame_extent_area_and_length_bounds() {
    let policy = TexturePolicy::for_device(64);
    assert_eq!(policy.source_bytes(8192, 64, true), Some(8192 * 64 * 4));
    assert_eq!(
        policy.source_bytes(8192, 64, true),
        policy.source_bytes(64, 8192, true)
    );
    assert_eq!(policy.source_bytes(8192, 64, false), None);
    assert_eq!(policy.source_bytes(8193, 64, true), None);
    assert_eq!(policy.source_bytes(8192, 65, true), None);
    assert_eq!(policy.source_bytes(u32::from(u16::MAX) + 1, 64, true), None);
    assert_eq!(
        TexturePolicy::for_device(MAX_TEXTURE_SIDE).source_bytes(4096, 2048, true),
        None
    );
}

#[test]
fn terrain_budget_counts_complete_mips_and_device_dimensions() {
    let policy = TexturePolicy::for_device(MAX_TEXTURE_SIDE);
    let bytes = policy.terrain_bytes(512, 2).unwrap();
    assert_eq!(
        bytes,
        (0..=9)
            .map(|mip| ((512usize >> mip).pow(2)) * 4 * 2)
            .sum::<usize>()
    );
    assert!(
        policy
            .terrain_bytes(512, MAX_SUBSCRIBER_BYTES / (512 * 512 * 4) + 1)
            .is_none()
    );
    assert!(policy.terrain_bytes(512, usize::MAX).is_none());
    assert!(
        TexturePolicy::for_device(256)
            .terrain_bytes(512, 1)
            .is_none()
    );
}

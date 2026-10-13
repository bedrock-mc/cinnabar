use super::*;

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

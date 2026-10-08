use super::*;
use world::{BLOCKS_PER_SUB_CHUNK, RawBlockIds};

const PORTAL: u32 = 7;
const ORDINARY: u32 = 1;

fn fixture(portal_position: Option<[usize; 3]>, secondary_portal: bool) -> SubChunk {
    let mut bytes = vec![8, if secondary_portal { 2 } else { 1 }, 3];
    let mut words = vec![0_u32; BLOCKS_PER_SUB_CHUNK / u32::BITS as usize];
    if let Some([x, y, z]) = portal_position {
        let linear = (x * SUB_CHUNK_SIDE + z) * SUB_CHUNK_SIDE + y;
        words[linear / u32::BITS as usize] |= 1 << (linear % u32::BITS as usize);
    }
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    // The deliberately unused palette entry must not create a surface.
    bytes.extend_from_slice(&[4, (ORDINARY * 2) as u8, (PORTAL * 2) as u8]);
    if secondary_portal {
        bytes.extend_from_slice(&[1, (PORTAL * 2) as u8]);
    }
    SubChunk::decode(&bytes, &RawBlockIds { air: 0 })
}

fn classify(id: u32) -> Option<BlockEntityKind> {
    (id == PORTAL).then_some(BlockEntityKind::EndPortal)
}

#[test]
fn primary_palette_portals_are_drawn_without_block_entity_nbt() {
    let side = SUB_CHUNK_SIDE as i32;
    let block = [-side + 2, side + 3, -side + 4];
    let eye = Vec3::from_array(block.map(|value| value as f32 + 0.5));
    let mut submissions = Vec::new();
    submit_sub_chunk(
        &mut submissions,
        [-1, 1, -1],
        &fixture(Some([2, 3, 4]), false),
        eye,
        &mut classify,
    );
    assert_eq!(submissions.len(), 1);
    assert_eq!(submissions[0].block, block);
    assert_eq!(submissions[0].kind, BlockEntityKind::EndPortal);
    assert_eq!(submissions[0].light, 1.0.into());
}

#[test]
fn unused_palette_entries_and_secondary_storage_cannot_create_portal_surfaces() {
    for secondary in [false, true] {
        let mut submissions = Vec::new();
        submit_sub_chunk(
            &mut submissions,
            [0; 3],
            &fixture(None, secondary),
            Vec3::ZERO,
            &mut classify,
        );
        assert!(submissions.is_empty());
    }
}

#[test]
fn portal_removal_immediately_removes_its_surface_and_preserves_submission_limits() {
    let portal = fixture(Some([0; 3]), false);
    let mut submissions = vec![
        BlockEntitySubmission {
            block: [0; 3],
            light: 1.0.into(),
            kind: BlockEntityKind::EndPortal,
        };
        super::super::MAX_SUBMISSIONS
    ];
    submit_sub_chunk(&mut submissions, [0; 3], &portal, Vec3::ZERO, &mut classify);
    assert_eq!(submissions.len(), super::super::MAX_SUBMISSIONS);
    submissions.clear();
    submit_sub_chunk(
        &mut submissions,
        [0; 3],
        &fixture(None, false),
        Vec3::ZERO,
        &mut classify,
    );
    assert!(submissions.is_empty());
    submit_sub_chunk(
        &mut submissions,
        [0; 3],
        &portal,
        Vec3::splat(super::super::SCAN_RADIUS_BLOCKS * 2.0),
        &mut classify,
    );
    assert!(submissions.is_empty());
}

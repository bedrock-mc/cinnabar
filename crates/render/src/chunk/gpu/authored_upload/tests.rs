use super::*;

fn array(side: u32, layers: u32) -> TextureArray {
    TextureArray {
        layers,
        mips: (0..=side.ilog2())
            .map(|level| {
                let size = side >> level;
                TextureMip {
                    size,
                    rgba8: vec![level as u8; (size * size * layers * 4) as usize]
                        .into_boxed_slice(),
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    }
}

#[test]
fn bounded_stripes_visit_each_page_mip_layer_and_row_exactly_once() {
    let arrays = [
        array(8, 3),
        array(4, 2),
        array(8, 3),
        array(4, 2),
        array(8, 3),
        array(4, 2),
    ];
    let pages = arrays.each_ref();
    let mut schedule = Schedule::new(&pages).unwrap();
    let mut visits = arrays.map(|array| {
        array
            .mips
            .iter()
            .map(|mip| vec![false; mip.rgba8.len()])
            .collect::<Vec<_>>()
    });
    let budget = 68;
    let mut frames = 0;
    while !schedule.complete() {
        let mut remaining = budget;
        let mut frame_bytes = 0;
        while let Some(stripe) = schedule.next(remaining) {
            assert!(stripe.bytes <= remaining);
            assert!(stripe.offset + stripe.bytes <= visits[stripe.page][stripe.mip as usize].len());
            for written in &mut visits[stripe.page][stripe.mip as usize]
                [stripe.offset..stripe.offset + stripe.bytes]
            {
                assert!(!*written, "each authored source byte must be uploaded once");
                *written = true;
            }
            remaining -= stripe.bytes;
            frame_bytes += stripe.bytes;
        }
        assert!(frame_bytes > 0 && frame_bytes <= budget);
        frames += 1;
    }
    assert!(frames > 1);
    assert!(visits.iter().flatten().flatten().all(|written| *written));
    assert!(schedule.next(budget).is_none());
}

#[test]
fn scheduler_does_not_advance_when_a_row_exceeds_the_budget() {
    let arrays = std::array::from_fn(|_| array(8, 1));
    let mut schedule = Schedule::new(&arrays.each_ref()).unwrap();
    assert!(schedule.next(31).is_none());
    let first = schedule.next(32).unwrap();
    assert_eq!(
        (first.page, first.layer, first.row, first.rows),
        (0, 0, 0, 1)
    );
    assert_eq!(first.bytes, 32);
}

#[test]
fn whole_layers_are_coalesced_without_padding_the_remaining_mips() {
    let arrays = std::array::from_fn(|_| array(4, 4));
    let mut schedule = Schedule::new(&arrays.each_ref()).unwrap();
    let first = schedule.next(128).unwrap();
    assert_eq!((first.layer_count, first.rows, first.bytes), (2, 4, 128));
    let second = schedule.next(128).unwrap();
    assert_eq!(
        (second.layer, second.layer_count, second.bytes),
        (2, 2, 128)
    );
    let third = schedule.next(128).unwrap();
    assert_eq!((third.mip, third.layer_count, third.bytes), (1, 4, 64));
}

#[test]
fn obsolete_authored_identity_cancels_without_changing_carrier_identity() {
    let current = ChunkTextureAssetIdentity {
        pointer: 1,
        enhanced_pointer: 2,
        revision: 7,
    };
    let replacement = ChunkTextureAssetIdentity {
        enhanced_pointer: 3,
        ..current
    };
    assert_ne!(current, replacement);
    assert!(same_carrier(current, replacement));
    assert!(!same_carrier(
        current,
        ChunkTextureAssetIdentity {
            revision: 8,
            ..replacement
        }
    ));
    assert!(!same_carrier(
        current,
        ChunkTextureAssetIdentity {
            pointer: 4,
            ..replacement
        }
    ));
    let arrays = std::array::from_fn(|_| array(8, 2));
    let mut obsolete = Schedule::new(&arrays.each_ref()).unwrap();
    obsolete.next(32).unwrap();
    let mut replacement = Schedule::new(&arrays.each_ref()).unwrap();
    let first = replacement.next(32).unwrap();
    assert_eq!(
        (first.page, first.mip, first.layer, first.row),
        (0, 0, 0, 0)
    );
}

#[test]
fn malformed_mip_shapes_are_rejected_before_any_gpu_upload() {
    let mut arrays = std::array::from_fn(|_| array(8, 1));
    arrays[2].mips[1].rgba8 = vec![0; 3].into_boxed_slice();
    assert!(Schedule::new(&arrays.each_ref()).is_none());
}

#[path = "native.rs"]
mod native;

use super::*;

#[test]
fn queue_mip_uploads_borrow_every_layer_without_padding() {
    for base_size in [16_u32, 128] {
        let texture = TextureArray {
            layers: 3,
            mips: (0..=base_size.ilog2())
                .map(|level| {
                    let size = base_size >> level;
                    TextureMip {
                        size,
                        rgba8: (0..size * size * 3)
                            .flat_map(|pixel| {
                                [
                                    (pixel / (size * size)) as u8,
                                    level as u8,
                                    (pixel % size) as u8,
                                    ((pixel / size) % size) as u8,
                                ]
                            })
                            .collect(),
                    }
                })
                .collect(),
        };
        let plans = plan_queue_texture_mips(&texture).unwrap();
        let mut visited = 0;
        let bytes = write_texture_mips(&texture, &plans, |plan, bytes| {
            let mip = &texture.mips[plan.mip_level as usize];
            assert_eq!(
                bytes.as_ptr(),
                mip.rgba8.as_ptr(),
                "queue writes borrow source bytes"
            );
            assert_eq!(plan.bytes_per_row, mip.size * 4);
            assert_eq!(plan.rows_per_image, mip.size);
            for layer in 0..texture.layers as usize {
                for row in 0..mip.size as usize {
                    let start =
                        (layer * plan.rows_per_image as usize + row) * plan.bytes_per_row as usize;
                    let end = start + mip.size as usize * 4;
                    assert_eq!(&bytes[start..end], &mip.rgba8[start..end]);
                }
            }
            visited += 1;
        });
        assert_eq!(visited, texture.mips.len());
        assert_eq!(
            bytes,
            texture
                .mips
                .iter()
                .map(|mip| mip.rgba8.len() as u64)
                .sum::<u64>()
        );
    }
}

use super::*;

/// Selects the positional bubble art and shows its surface only below primary air.
pub(super) fn select_template(
    context: PaletteResolutionContext<'_, '_>,
    coordinate: [usize; 3],
    entry: ResolvedPaletteEntry,
) -> u32 {
    let origin = context.neighbourhood.block_origin();
    let position = std::array::from_fn(|axis| origin[axis].wrapping_add(coordinate[axis] as i32));
    let [x, y, z] = coordinate.map(|value| value as i32);
    let exposed = match context.neighbourhood.sample(0, [x, y + 1, z]) {
        world::MeshSample::Open => true,
        world::MeshSample::Block(id) => context.classifier.is_air(id),
    };
    entry.model_template
        + sprite_variant(position)
        + if exposed {
            assets::BUBBLE_COLUMN_POSITION_VARIANTS
        } else {
            0
        }
}

/// Returns the low two bits of the coordinate-byte selector shared by the shell and surface.
fn sprite_variant([x, _, z]: [i32; 3]) -> u32 {
    let hash = |word: i32| {
        word.to_le_bytes()
            .into_iter()
            .fold(0x8422_2325_u32, |hash, byte| {
                (hash ^ u32::from(byte)).wrapping_mul(0x1b3)
            })
    };
    (hash(x) ^ hash(z)) & (assets::BUBBLE_COLUMN_POSITION_VARIANTS - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positional_sprites_include_high_bytes_and_ignore_height() {
        for (x, z, expected) in [
            (0, 0, 0),
            (1, 0, 1),
            (0, 2, 2),
            (3, 0, 3),
            (256, 0, 3),
            (-1, 0, 0),
            (-256, 0, 3),
        ] {
            for y in [i32::MIN, 0, i32::MAX] {
                assert_eq!(sprite_variant([x, y, z]), expected, "x={x},z={z}");
            }
        }
    }
}

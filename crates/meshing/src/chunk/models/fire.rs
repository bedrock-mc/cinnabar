use super::*;

pub(super) fn select_template<'a>(
    context: PaletteResolutionContext<'_, 'a>,
    facts: &PaletteFacts<'a>,
    neighbour_facts: &[OnceCell<PaletteFacts<'a>>; Face::ALL.len()],
    coordinate: [usize; 3],
    entry: ResolvedPaletteEntry,
) -> u32 {
    let neighbour =
        |face| adjacent_palette_entry(context, facts, neighbour_facts, coordinate, face);
    let below = neighbour(Face::NegativeY);
    if below
        .flags
        .intersects(BlockFlags::FIRE_TOP_SUPPORT | BlockFlags::FIRE_FLAMMABLE)
    {
        return entry.model_template;
    }
    let faces = [
        Face::NegativeX,
        Face::PositiveX,
        Face::NegativeZ,
        Face::PositiveZ,
        Face::PositiveY,
    ];
    let mask = faces
        .into_iter()
        .enumerate()
        .fold(0, |mask, (index, face)| {
            mask | (u8::from(neighbour(face).flags.contains(BlockFlags::FIRE_FLAMMABLE)) << index)
        });
    let origin = context.neighbourhood.block_origin();
    let position =
        std::array::from_fn::<_, 3, _>(|axis| origin[axis].wrapping_add(coordinate[axis] as i32));
    let alternate = position.into_iter().fold(0_i32, i32::wrapping_add) & 1 != 0;
    // Native signed division truncates toward zero, including negative positions.
    let flip_u = position
        .into_iter()
        .map(|value| value / 2)
        .fold(0_i32, i32::wrapping_add)
        & 1
        != 0;
    entry.model_template + assets::fire_attachment_template_offset(mask, alternate, flip_u)
}

use super::*;

/// Combines the lower selectors and upper hinge only within one admitted door class.
pub(super) fn select_template<'a>(
    context: PaletteResolutionContext<'_, 'a>,
    facts: &PaletteFacts<'a>,
    neighbour_facts: &[OnceCell<PaletteFacts<'a>>; Face::ALL.len()],
    coordinate: [usize; 3],
    entry: ResolvedPaletteEntry,
) -> Option<u32> {
    assets::door::resolve_template(
        context.visuals,
        entry.model_template,
        entry.variant,
        |upper| {
            let neighbor = adjacent_palette_entry(
                context,
                facts,
                neighbour_facts,
                coordinate,
                if upper {
                    Face::NegativeY
                } else {
                    Face::PositiveY
                },
            );
            (neighbor.kind == VisualKind::Model)
                .then_some((neighbor.model_template, neighbor.variant))
        },
    )
}

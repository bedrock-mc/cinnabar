use super::state::State;
use assets::facts::BlockProperty;
use assets::{RegistryRecord, block_shapes::ShapeFlags};

/// Reads compiled support facts independently of movement geometry.
pub(super) fn flags(record: &RegistryRecord) -> ShapeFlags {
    let traits = record.traits;
    let mut flags = ShapeFlags::empty();
    flags.set(
        ShapeFlags::VINE_SUPPORT,
        traits.contains(assets::registry::BlockTraits::VINE_SUPPORT),
    );
    flags.set(
        ShapeFlags::FULL_TOP_SUPPORT,
        traits.contains(assets::registry::BlockTraits::FULL_TOP_SUPPORT),
    );
    flags
}

/// Selects the neighbour rule and its compiled alternate geometry.
pub(super) fn selection(
    record: &RegistryRecord,
) -> (
    assets::block_shapes::SelectionContext,
    Box<[assets::block_shapes::ShapeBox]>,
) {
    use assets::block_shapes::{SelectionContext, ShapeBox};
    match () {
        _ if record.model_family == assets::ModelFamily::Vine
            && State::read(record)
                .and_then(|state| state.number(BlockProperty::VineDirectionBits))
                == Some(0) =>
        {
            (
                SelectionContext::VineSupport,
                Box::new([
                    ShapeBox::new([0.0, 0.9375, 0.0], [1.0; 3]),
                    ShapeBox::new([0.0; 3], [1.0, 0.0625, 1.0]),
                    super::UNIT,
                ]),
            )
        }
        _ if record
            .traits
            .contains(assets::registry::BlockTraits::SCAFFOLDING) =>
        {
            let mut boxes = super::scaffolding::selection(record).unwrap().into_vec();
            boxes.extend([
                ShapeBox::new([0.125, 0.0, 0.0], [0.875, 0.125, 0.125]),
                ShapeBox::new([0.125, 0.0, 0.875], [0.875, 0.125, 1.0]),
                ShapeBox::new([0.0, 0.0, 0.125], [0.125, 0.125, 0.875]),
                ShapeBox::new([0.875, 0.0, 0.125], [1.0, 0.125, 0.875]),
            ]);
            (SelectionContext::ScaffoldSupport, boxes.into_boxed_slice())
        }
        _ if record
            .traits
            .contains(assets::registry::BlockTraits::PORTAL)
            && !record
                .traits
                .contains(assets::registry::BlockTraits::END_PORTAL)
            && State::read(record).and_then(|state| {
                state
                    .text(BlockProperty::PortalAxis)
                    .map(|axis| axis == "unknown")
            }) == Some(true) =>
        {
            (
                SelectionContext::PortalAxis,
                Box::new([ShapeBox::new([0.0, 0.0, 0.375], [1.0, 1.0, 0.625])]),
            )
        }
        _ => (SelectionContext::Static, Box::new([])),
    }
}

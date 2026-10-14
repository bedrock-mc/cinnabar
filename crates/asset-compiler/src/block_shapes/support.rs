use super::state::State;
use assets::{RegistryRecord, block_shapes::ShapeFlags};

/// Reads the shared compiled support facts, deriving only historical compiler inputs.
pub(super) fn flags(record: &RegistryRecord) -> ShapeFlags {
    let traits = if record.traits.is_empty() {
        crate::registry_policy::block_support(record)
    } else {
        record.traits
    };
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
    match record.name.as_ref() {
        "minecraft:vine"
            if State::read(record).and_then(|state| state.number("vine_direction_bits"))
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
        "minecraft:scaffolding" => {
            let mut boxes = super::scaffolding::selection(record).unwrap().into_vec();
            boxes.extend([
                ShapeBox::new([0.125, 0.0, 0.0], [0.875, 0.125, 0.125]),
                ShapeBox::new([0.125, 0.0, 0.875], [0.875, 0.125, 1.0]),
                ShapeBox::new([0.0, 0.0, 0.125], [0.125, 0.125, 0.875]),
                ShapeBox::new([0.875, 0.0, 0.125], [1.0, 0.125, 0.875]),
            ]);
            (SelectionContext::ScaffoldSupport, boxes.into_boxed_slice())
        }
        "minecraft:portal"
            if State::read(record)
                .and_then(|state| state.text("portal_axis").map(|axis| axis == "unknown"))
                == Some(true) =>
        {
            (
                SelectionContext::PortalAxis,
                Box::new([ShapeBox::new([0.0, 0.0, 0.375], [1.0, 1.0, 0.625])]),
            )
        }
        _ => (SelectionContext::Static, Box::new([])),
    }
}

use super::state::State;
use assets::block_shapes::CompiledDoor;
use assets::facts::BlockProperty;
use assets::{RegistryRecord, registry::BlockTraits};

/// Decodes paired-door properties from the same record that supplies the runtime ID.
pub(super) fn state(record: &RegistryRecord) -> Option<CompiledDoor> {
    if !record.traits.contains(BlockTraits::DOOR) {
        return None;
    }
    let state = State::read(record)?;
    Some(CompiledDoor {
        family: world::block_state_identity::state_identity(&record.name, []),
        facing: u8::try_from(state.direction()?).ok()?,
        upper: state.bit(BlockProperty::UpperBlockBit)?,
        open: state.bit(BlockProperty::OpenBit)?,
        hinge_right: state.bit(BlockProperty::DoorHingeBit)?,
    })
}

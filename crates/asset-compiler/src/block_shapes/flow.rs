//! Vanilla material/flow facts per block; geometry flags do not establish them.

use assets::block_shapes::CompiledFlow;
use assets::{ModelStateField, RegistryRecord};
use assets::{facts::shape::ShapeTraits, registry::BlockTraits};

/// Compiles the admitted material and liquid-depth facts for a state.
pub(super) fn facts(record: &RegistryRecord, shapes: ShapeTraits) -> Option<CompiledFlow> {
    let liquid = record
        .traits
        .intersects(BlockTraits::WATER | BlockTraits::LAVA)
        && !shapes.contains(ShapeTraits::BUBBLE_COLUMN);
    let solid = shapes.contains(ShapeTraits::SOLID_FLOW);
    if !liquid && !solid && !record.traits.contains(BlockTraits::AIR) {
        return None;
    }
    let liquid_depth = if liquid {
        let depth = record
            .model_state
            .get(ModelStateField::LiquidDepth)
            .and_then(|depth| u8::try_from(depth).ok())
            .filter(|depth| *depth < 16)?;
        Some(depth)
    } else {
        None
    };
    Some(CompiledFlow {
        blocks_motion: solid,
        is_solid: solid,
        // These blocks use vanilla's default liquid detection (no blocked
        // faces) and allow flow on every face.
        blocked_faces: 0,
        allowed_faces: 0x3f,
        liquid_depth,
    })
}

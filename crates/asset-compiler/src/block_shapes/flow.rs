//! Vanilla material/flow facts per block; geometry flags do not establish them.

use assets::block_shapes::CompiledFlow;
use assets::{ModelStateField, RegistryRecord};

/// Compiles the admitted material and liquid-depth facts for a state.
pub(super) fn facts(record: &RegistryRecord) -> Option<CompiledFlow> {
    let (blocks_motion, is_solid, liquid) = match record.name.as_ref() {
        // Air and liquid materials neither block motion nor count as solid.
        "minecraft:air" => (false, false, false),
        "minecraft:water"
        | "minecraft:flowing_water"
        | "minecraft:lava"
        | "minecraft:flowing_lava" => (false, false, true),
        // Dirt and grass use a solid, motion-blocking material.
        "minecraft:dirt" | "minecraft:grass" | "minecraft:grass_block" => (true, true, false),
        // Both ice materials are solid.
        "minecraft:ice" | "minecraft:packed_ice" => (true, true, false),
        // Stone and sand share a solid, motion-blocking material.
        "minecraft:stone" | "minecraft:sand" => (true, true, false),
        // Gravel uses the same material; it falls with falling_dust_gravel_particle.
        "minecraft:gravel" => (true, true, false),
        _ => return None,
    };
    let liquid_depth = if liquid {
        let Some(depth) = record
            .model_state
            .get(ModelStateField::LiquidDepth)
            .and_then(|depth| u8::try_from(depth).ok())
            .filter(|depth| *depth < 16)
        else {
            return None;
        };
        Some(depth)
    } else {
        None
    };
    Some(CompiledFlow {
        blocks_motion,
        is_solid,
        // These blocks use vanilla's default liquid detection (no blocked
        // faces) and allow flow on every face.
        blocked_faces: 0,
        allowed_faces: 0x3f,
        liquid_depth,
    })
}

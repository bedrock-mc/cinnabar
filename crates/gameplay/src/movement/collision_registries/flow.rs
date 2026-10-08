//! Vanilla material/flow facts per block; geometry flags do not establish them.

use assets::{ModelStateField, RegistryRecord};
use sim::{CollisionRegistry, FlowBlockFacts};

pub(super) fn register(registry: &mut CollisionRegistry, runtime_id: u32, record: &RegistryRecord) {
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
        _ => return,
    };
    let liquid_depth = if liquid {
        let Some(depth) = record
            .model_state
            .get(ModelStateField::LiquidDepth)
            .and_then(|depth| u8::try_from(depth).ok())
            .filter(|depth| *depth < 16)
        else {
            return;
        };
        Some(depth)
    } else {
        None
    };
    registry.set_flow_facts(
        runtime_id,
        FlowBlockFacts {
            blocks_motion,
            is_solid,
            // These blocks use vanilla's default liquid detection (no blocked
            // faces) and allow flow on every face.
            blocked_faces: 0,
            allowed_faces: 0x3f,
            liquid_depth,
        },
    );
}

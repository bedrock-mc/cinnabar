//! Native material/flow cache bindings; geometry flags do not establish them.

use assets::{ModelStateField, RegistryRecord};
use sim::{CollisionRegistry, FlowBlockFacts};

pub(super) fn register(registry: &mut CollisionRegistry, runtime_id: u32, record: &RegistryRecord) {
    let (blocks_motion, is_solid, liquid) = match record.name.as_ref() {
        // Material::_setupMaterials current 0x0379bed0: types 0 / 5 / 6.
        "minecraft:air" => (false, false, false),
        "minecraft:water"
        | "minecraft:flowing_water"
        | "minecraft:lava"
        | "minecraft:flowing_lava" => (false, false, true),
        // DirtBlock 0x0a7b9820 / GrassBlockBase 0x0712ab00 use type 1.
        "minecraft:dirt" | "minecraft:grass" | "minecraft:grass_block" => (true, true, false),
        // IceBlock current 0x071305c0 chooses types 13 / 23 (both solid).
        "minecraft:ice" | "minecraft:packed_ice" => (true, true, false),
        // Current StoneBlock 0x0a5b7aa0 / SandBlock 0x08efbad0 use type 23;
        // recovered registerBlock wrappers are 0x0dfb6010 / 0x0dfb97a0.
        "minecraft:stone" | "minecraft:sand" => (true, true, false),
        // GravelBlock 0x0712be60 also uses type 23; its vtable 0x1502a5290
        // selects the native falling_dust_gravel_particle producer.
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
            // These identified native classes use the default liquid detection
            // cache (mask 0) and BlockType directional virtual (always true).
            blocked_faces: 0,
            allowed_faces: 0x3f,
            liquid_depth,
        },
    );
}

//! Compiler memberships for geometry stored in the physics carrier.

use bitflags::bitflags;

bitflags! {
    /// Shape classes resolved by registryc before compiling numeric geometry.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct ShapeTraits: u128 {
        const BARRIER = 1 << 0;
        const END_STONE = 1 << 1;
        const PALE_MOSS = 1 << 2;
        const BUBBLE_COLUMN = 1 << 3;
        const NEVER_PICK = 1 << 4;
        const WALL_BANNER = 1 << 5;
        const SOLID_FLOW = 1 << 6;
        const BELL = 1 << 7;
        const STONECUTTER = 1 << 8;
        const FLOWER_POT = 1 << 9;
        const SNIFFER_EGG = 1 << 10;
        const WATERLILY = 1 << 11;
        const HEAVY_CORE = 1 << 12;
        const ENCHANTING_TABLE = 1 << 13;
        const DAYLIGHT_DETECTOR = 1 << 14;
        const DECORATED_POT = 1 << 15;
        const DRIED_GHAST = 1 << 16;
        const SCULK_SENSOR = 1 << 17;
        const CAKE = 1 << 18;
        const REPEATER = 1 << 19;
        const CANDLE_CAKE = 1 << 20;
        const SHELF = 1 << 21;
        const PLANT_UNIT = 1 << 22;
        const FROG_SPAWN = 1 << 23;
        const PETALS = 1 << 24;
        const LEAF_LITTER = 1 << 25;
        const SEA_PICKLE = 1 << 26;
        const CRIMSON_FUNGUS = 1 << 27;
        const WARPED_FUNGUS = 1 << 28;
        const KELP = 1 << 29;
        const GRASS = 1 << 30;
        const FLOWER = 1 << 31;
        const PITCHER_PLANT = 1 << 32;
        const HANGING_ROOTS = 1 << 33;
        const SPORE_BLOSSOM = 1 << 34;
        const BERRY_BUSH = 1 << 35;
        const PROPAGULE = 1 << 36;
        const STEM = 1 << 37;
        const PITCHER_CROP = 1 << 38;
        const CORAL = 1 << 39;
        const CORAL_FAN = 1 << 40;
        const CORAL_WALL_FAN = 1 << 41;
        const SHORT_DRY_GRASS = 1 << 42;
        const MUSHROOM = 1 << 43;
        const NETHER_SPROUTS = 1 << 44;
        const CACTUS_FLOWER = 1 << 45;
        const REEDS = 1 << 46;
        const BUSH = 1 << 47;
        const DOUBLE_PLANT = 1 << 48;
        const VISUAL_UNIT = 1 << 49;
        const HONEY = 1 << 50;
        const PATH = 1 << 51;
        const LECTERN = 1 << 52;
        const CONDUIT = 1 << 53;
        const END_PORTAL_FRAME = 1 << 54;
        const BAMBOO = 1 << 55;
        const TALL_DRY_GRASS = 1 << 56;
        const TORCHFLOWER_CROP = 1 << 57;
        const TURTLE_EGG = 1 << 58;
        const BIG_DRIPLEAF = 1 << 59;
        const CHORUS_PLANT = 1 << 60;
        const TWISTING_VINES = 1 << 61;
        const WEEPING_VINES = 1 << 62;
        const CAVE_VINES = 1 << 63;
        const SMALL_DRIPLEAF = 1 << 64;
        const WHEAT = 1 << 65;
        const CARROTS = 1 << 66;
        const ROOT_CROP = 1 << 67;
        const NETHER_WART = 1 << 68;
        const CACTUS = 1 << 69;
        const BREWING_STAND = 1 << 70;
        const HANGING_MOSS = 1 << 71;
        const ANVIL = 1 << 72;
        const PISTON_ARM = 1 << 73;
        const MOVING_BLOCK = 1 << 74;
        const REDSTONE_WIRE = 1 << 75;
        const TRIP_WIRE = 1 << 76;
        const TRIPWIRE_HOOK = 1 << 77;
        const END_GATEWAY = 1 << 78;
        const SHELF_MUSHROOM = 1 << 79;
    }
}

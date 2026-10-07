//! Guest SDK generated from the same WIT contract used by the host.

/// Ticks in one Bedrock day, shared by capability validation and sky math.
pub const BEDROCK_DAY_TICKS: u32 = 24_000;
/// Maximum remote players exposed by one local gameplay snapshot.
pub const MAX_GAMEPLAY_PLAYERS: usize = 128;
/// Maximum accumulated camera change per axis in one callback, in radians.
pub const MAX_CAMERA_DELTA_RADIANS: f32 = 0.25;
/// Personal attack overrides never extend actor selection beyond this local bound.
pub const MAX_ENTITY_REACH_BLOCKS: f32 = 6.0;
/// Maximum opt-in delay of post-login application packets in either direction.
pub const MAX_PACKET_DELAY_MS: u32 = 1_000;
/// Bounded control and settings payloads for personal components.
pub const MAX_SETTINGS_BYTES: usize = 16 * 1024;
pub const MAX_CONTROL_KEYS: usize = 64;
/// Maximum bytes in a physical key name supplied by local controls.
pub const MAX_CONTROL_KEY_BYTES: usize = 32;
/// Maximum nearby mobs in one snapshot, and the radius they are drawn from.
pub const MAX_GAMEPLAY_MOBS: usize = 64;
pub const MAX_MOB_RANGE_BLOCKS: f32 = 64.0;
pub const MAX_MOB_TYPE_BYTES: usize = 64;
/// Camera rig bounds: |right| and up/down offset, boom length, |roll| and |FOV change|.
pub const MAX_RIG_SIDE_BLOCKS: f32 = 2.0;
pub const MAX_RIG_VERTICAL_BLOCKS: f32 = 2.0;
pub const MAX_RIG_BACK_BLOCKS: f32 = 8.0;
pub const MAX_RIG_ROLL_RADIANS: f32 = 0.6;
pub const MAX_RIG_FOV_DELTA_DEGREES: f32 = 30.0;
/// Granted command names, their byte bound, and per-frame command requests.
pub const MAX_COMMAND_GRANTS: usize = 8;
pub const MAX_COMMAND_BYTES: usize = 128;
pub const MAX_COMMANDS_PER_FRAME: usize = 4;
/// Command requests accepted per second of gameplay frame time.
pub const MAX_COMMANDS_PER_SECOND: usize = 10;
/// Presentation cue bounds per frame.
pub const MAX_CUES_PER_FRAME: usize = 16;
pub const MAX_CUE_NAME_BYTES: usize = 32;
pub const MAX_CUE_VALUES: usize = 8;
/// Cues delivered to one callback from every loaded mod.
pub const MAX_INCOMING_CUES: usize = 64;
/// Local mods running at once.
pub const MAX_LOADED_MODS: usize = 4;

/// Render capability budgets, per instance or per committed callback.
pub const MAX_RENDER_PASSES: usize = 8;
/// Shader validations allowed in one frame callback; `init` may compile every pass.
pub const MAX_PASS_COMPILES_PER_FRAME: u32 = 1;
pub const MAX_PASS_NAME_BYTES: usize = 32;
pub const MAX_PASS_PARAMS: usize = 16;
pub const MAX_SHADER_BYTES: usize = 16 * 1024;
/// Worst-case texture reads and IR expressions one fragment may execute, helpers included.
pub const MAX_SHADER_TEXTURE_SAMPLES: u32 = 32;
pub const MAX_SHADER_EXPRESSIONS: u32 = 4096;
/// Bounds expression nesting, which is otherwise one level per operator in a statement.
pub const MAX_STATEMENT_TOKENS: usize = 512;
/// Largest value any shader type may hold, which bounds per-pixel private memory.
pub const MAX_SHADER_TYPE_BYTES: u32 = 1024;
pub const MAX_RENDER_DECALS: usize = 64;
pub const MAX_RENDER_RIBBONS: usize = 32;
pub const MAX_RIBBON_POINTS: usize = 64;
pub const MAX_RENDER_BEAMS: usize = 16;
pub const MAX_RENDER_BILLBOARDS: usize = 512;
/// Largest decal radius, ribbon or beam width, or billboard side, in blocks.
pub const MAX_PRIMITIVE_EXTENT_BLOCKS: f32 = 64.0;
/// Primitives farther than this from the origin of either axis are rejected.
pub const MAX_PRIMITIVE_COORDINATE: f32 = 30_000_000.0;

/// Bounds for retained loaded-block highlighting.
pub const MAX_BLOCK_HIGHLIGHTS: usize = 1024;
pub const MAX_BLOCK_HIGHLIGHT_IDENTIFIERS: usize = 8;
pub const MAX_BLOCK_HIGHLIGHT_IDENTIFIER_BYTES: usize = 128;
pub const MAX_BLOCK_HIGHLIGHT_RANGE: f32 = 128.0;

/// Host-owned selection; block coordinates never cross the guest boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockHighlightSpec {
    pub identifiers: Vec<String>,
    pub range: f32,
    pub color: [f32; 4],
}

pub mod bindings {
    wit_bindgen::generate!({
        path: "wit",
        world: "extension",
        pub_export_macro: true,
    });
}

/// Versioned SDK for consented server components, separate from personal mods.
pub mod server_bundle {
    wit_bindgen::generate!({
        path: "wit",
        world: "server-bundle",
        generate_all,
        pub_export_macro: true,
        export_macro_name: "export_server_bundle",
    });
}

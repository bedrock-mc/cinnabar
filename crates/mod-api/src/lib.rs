//! Guest SDK for player mods, generated from the same `extension` WIT contract used by the host.
//! A server Experience's client part uses `experience-sdk`'s `client` feature instead.

/// Largest terrain query extent along each axis, in blocks.
pub const MAX_TERRAIN_EXTENT: u8 = 16;
/// Maximum cells in one terrain query.
pub const MAX_TERRAIN_CELLS: usize = (MAX_TERRAIN_EXTENT as usize).pow(3);
/// Maximum distinct terrain queries serviced for one mod per frame.
pub const MAX_TERRAIN_QUERIES: usize = 4;
/// Maximum terrain distance from the current player, along each axis.
pub const MAX_TERRAIN_DISTANCE: i64 = 128;
/// Cells re-sampled per frame across all mods; further changed queries wait a frame.
pub const MAX_TERRAIN_FRAME_CELLS: usize = 2 * MAX_TERRAIN_CELLS;
/// Distinct block-local boxes in one terrain region.
pub const MAX_TERRAIN_BOXES: usize = 4096;
/// Identifier, canonical-state and tag bytes in one terrain region.
pub const MAX_TERRAIN_METADATA_BYTES: usize = 256 * 1024;
/// Guest-memory bytes of one lowered terrain region.
pub const MAX_TERRAIN_RESPONSE_BYTES: usize = 512 * 1024;

mod guest;
pub mod native_camera;
pub mod native_frame;
pub mod native_menu;

/// Converts observed actor YXZ degrees (forward -Z, pitch up) to Bedrock view degrees.
pub fn bedrock_view_angles(yaw: f32, pitch: f32) -> [f32; 2] {
    [(180. - yaw).rem_euclid(360.), -pitch]
}

pub use guest::PlayerMod;

/// Exclusive bound for guest time overrides: ticks in one Bedrock day.
pub const BEDROCK_DAY_TICKS: u32 = 24_000;
/// Maximum remote players exposed by one local gameplay snapshot.
pub const MAX_GAMEPLAY_PLAYERS: usize = 128;
/// Fixed player inventory and worn armor lengths in local player-state snapshots.
pub const PLAYER_STATE_INVENTORY_SLOTS: usize = 36;
pub const PLAYER_STATE_ARMOR_SLOTS: usize = 4;
/// Bounds for read-only local player-state payloads.
pub const MAX_PLAYER_STATE_EFFECTS: usize = 32;
pub const MAX_ITEM_IDENTIFIER_BYTES: usize = 128;
/// Maximum accumulated camera change per axis in one callback, in radians.
pub const MAX_CAMERA_DELTA_RADIANS: f32 = 0.25;
/// Personal attack overrides never extend actor selection beyond this local bound.
pub const MAX_ENTITY_REACH_BLOCKS: f32 = 6.0;
/// Maximum opt-in delay of post-login application packets in either direction.
pub const MAX_PACKET_DELAY_MS: u32 = 1_000;
/// Bounded control and settings payloads for personal components.
pub const MAX_SETTINGS_BYTES: usize = 16 * 1024;
/// Inventory requests one player-mod callback may make.
pub const MAX_INVENTORY_REQUESTS: usize = 4;
/// Optional top-level component data containing one existing personal panel bundle.
pub const PANEL_BUNDLE_SECTION: &str = "cinnabar:extension/panel-bundle@1";
/// Optional top-level component data containing one outline font for the personal panel.
pub const PANEL_FONT_SECTION: &str = "cinnabar:extension/font@1";
/// Maximum source bytes in an embedded or explicitly selected personal panel font.
pub const MAX_PANEL_FONT_BYTES: usize = 2 * 1024 * 1024;
/// Nominal outline-font em size in personal-panel GUI coordinates.
pub const PANEL_FONT_EM_GUI_PIXELS: f32 = 9.0;
/// First private-use character reserved for optional personal-panel icon glyphs.
pub const PANEL_ICON_GLYPH_START: u32 = 0xE800;
/// Maximum number of dense icon glyphs in one personal-panel font.
pub const MAX_PANEL_ICON_GLYPHS: u32 = 64;
/// Optional top-level PNG data replacing the local main-menu logo.
pub const MENU_LOGO_SECTION: &str = "cinnabar:extension/menu-logo@1";
/// Optional top-level UTF-8 text replacing the local menu's product heading.
pub const MENU_TITLE_SECTION: &str = "cinnabar:extension/menu-title@1";
/// Optional top-level UTF-8 copyright and license notices, retained outside executable bytes.
pub const NOTICES_SECTION: &str = "cinnabar:extension/notices@1";
/// Bounded static menu-logo source and decoded image geometry.
pub const MAX_MENU_LOGO_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_MENU_LOGO_WIDTH: u32 = 4096;
pub const MAX_MENU_LOGO_HEIGHT: u32 = 2048;
pub const MAX_MENU_LOGO_PIXELS: u64 = 4 * 1024 * 1024;
/// A menu heading is one bounded, nonempty line of UTF-8 text.
pub const MAX_MENU_TITLE_BYTES: usize = 64;
/// Retained license text has its own static source budget.
pub const MAX_NOTICES_BYTES: usize = 4 * 1024 * 1024;
/// Optional top-level PNG shown beside the component in the Mods library.
pub const ICON_SECTION: &str = "cinnabar:extension/icon@1";
/// Optional top-level HTTPS GitHub repository URL enabling mod release notifications.
pub const UPDATE_REPOSITORY_SECTION: &str = "cinnabar:extension/update-repository@1";
/// Optional stable GitHub latest-release WASM URL enabling automatic mod installation.
pub const UPDATE_URL_SECTION: &str = "cinnabar:extension/update-url@1";
/// Bounded icon source and decoded side length; an icon outside them shows the placeholder.
pub const MAX_ICON_BYTES: usize = 256 * 1024;
pub const MAX_ICON_SIDE: u32 = 256;
/// Every presentation asset section stripped from executable component bytes.
pub const COMPONENT_ASSET_SECTIONS: [&str; 6] = [
    PANEL_BUNDLE_SECTION,
    PANEL_FONT_SECTION,
    MENU_LOGO_SECTION,
    MENU_TITLE_SECTION,
    NOTICES_SECTION,
    ICON_SECTION,
];
pub const MAX_CONTROL_KEYS: usize = 64;
/// Maximum bytes in a physical key name supplied by local controls.
pub const MAX_CONTROL_KEY_BYTES: usize = 32;
/// Whether a physical key name fits the shared controls wire contract.
pub fn control_key_valid(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_CONTROL_KEY_BYTES
        && key.bytes().all(|c| c.is_ascii_alphanumeric())
}

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
/// Lower bounds for current-frame FOV and routed look multipliers; their upper bound is 1.
pub const MIN_VIEW_FOV_SCALE: f32 = 0.1;
pub const MIN_VIEW_LOOK_SCALE: f32 = 0.05;
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
/// Render and retained drawing imports allowed in one callback, including denied calls.
pub const MAX_RENDER_CALLS: usize = 32;
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

/// The `extension` world: HUD label, input, visual time, gameplay, panel, settings, events and
/// render. The package also defines `player-mod`, whose types and links reach other packages.
pub mod bindings {
    wit_bindgen::generate!({
        path: [
            "../experience-sdk/wit/client/deps/server-experience",
            "../experience-sdk/wit/session",
            "wit",
        ],
        world: "cinnabar:extension/extension@0.1.0",
        generate_all,
        pub_export_macro: true,
    });
}

/// The `player-mod` world: `extension` plus screens beside the container screens, the
/// session's items and recipes, declared keys and event callbacks.
#[allow(
    clippy::too_many_arguments,
    reason = "wit-bindgen flattens screen-layout records into canonical ABI parameters"
)]
pub mod player_mod {
    wit_bindgen::generate!({
        path: [
            "../experience-sdk/wit/client/deps/server-experience",
            "../experience-sdk/wit/session",
            "wit",
        ],
        world: "cinnabar:extension/player-mod@0.1.0",
        generate_all,
        pub_export_macro: true,
        export_macro_name: "export_player_mod",
    });
}

/// The `packet-mod` world: a trusted mod that relays every session packet.
#[allow(
    clippy::too_many_arguments,
    reason = "wit-bindgen flattens the start identity into canonical ABI parameters"
)]
pub mod packet_mod {
    wit_bindgen::generate!({
        path: [
            "../experience-sdk/wit/client/deps/server-experience",
            "../experience-sdk/wit/session",
            "wit",
        ],
        world: "cinnabar:extension/packet-mod@0.1.0",
        pub_export_macro: true,
        export_macro_name: "export_packet_mod",
    });
}

/// Maximum bytes in a stable selected world or server identity.
pub const MAX_WORLD_IDENTITY_BYTES: usize = 256;

/// Bounded cosmetic actor and combat observations per sandbox callback.
pub const MAX_OBSERVED_ACTORS: usize = 16;
pub const MAX_OBSERVED_ACTOR_BOXES: usize = 8;
pub const MAX_OBSERVED_ACTOR_NAME_BYTES: usize = 128;
pub const MAX_OBSERVED_COMBAT_EVENTS: usize = 64;
pub const MAX_COMBAT_READ_EVENTS: usize = 4;

/// Total regular frame-callback fuel shared across all local mods.
pub const TOTAL_MOD_FRAME_FUEL: u64 = 200_000;

/// World drawing authority and generation-safe resource tokens.
pub mod drawing;

/// Retained drawing budgets per owner; aggregate capacity is reserved for every loaded mod.
pub const MAX_DRAW_INSTANCES: usize = 1024;
pub const MAX_DRAW_GEOMETRIES: usize = 256;
pub const MAX_DRAW_VERTICES: usize = 65_536;
pub const MAX_DRAW_INDICES: usize = 196_608;
pub const MAX_DRAW_GEOMETRY_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_DRAW_MUTATION_BYTES: usize = 1024 * 1024;
pub const MAX_DRAW_GROUP_NAME_BYTES: usize = 32;

/// Bounds host-expanded GPU topology independently of source vertex and index counts.
pub const MAX_DRAW_EXPANDED_VERTICES: usize = MAX_DRAW_INDICES;

/// Current actor observations and retained actor references, per drawing owner.
pub const MAX_DRAW_ENTITIES: usize = MAX_OBSERVED_ACTORS;

/// Caps submitted vertex work as well as resident mesh sizes, including repeated instances.
pub const MAX_DRAW_VERTEX_WORK: usize = MAX_DRAW_EXPANDED_VERTICES * 4;

/// Largest linear RGBA component admitted by retained unlit drawing.
pub const MAX_DRAW_COLOR_COMPONENT: f32 = 1.0;

/// Procedural drawing presets and the fixed material parameter contract.
pub mod materials;
/// Retained custom materials per owner, including renderer-retiring resources.
pub const MAX_DRAW_MATERIALS: usize = 8;
/// Custom material validation attempts per callback, including rejected sources.
pub const MAX_MATERIAL_COMPILES_PER_CALLBACK: u32 = 1;
/// New native material pipeline compilations across all mods in one render frame.
pub const MAX_MATERIAL_PIPELINE_COMPILES_PER_FRAME: usize = 1;
/// Material admission uses the same source and expression ceilings as post effects.
pub const MAX_MATERIAL_SHADER_BYTES: usize = MAX_SHADER_BYTES;
pub const MAX_MATERIAL_SHADER_EXPRESSIONS: u32 = MAX_SHADER_EXPRESSIONS;
/// Largest linear emission component delivered to the world HDR pipeline.
pub const MAX_MATERIAL_EMISSION: f32 = 16.0;

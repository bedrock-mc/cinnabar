// BEGIN GENERATED EXTENSION CONTRACT
// Generated from crates/server-experience/schema/extension.json; do not edit.
pub const MARKER_PATH: &str = "cinnabar/extension-offer.json";

pub const WIRE_VERSION: u16 = 1;

pub const MAX_WIRE_VERSION: u16 = 2;

pub const API_VERSION: u16 = 1;

pub const MAX_IDENTIFIER_BYTES: usize = 96;

pub const MAX_URL_BYTES: usize = 2048;

pub const MAX_ORIGINS: usize = 8;

pub const MAX_FALLBACK_BYTES: usize = 512;

pub const MAX_MARKER_BYTES: usize = 64 * 1024;

pub const MAX_PAYLOAD_BYTES: usize = 16 * 1024;

pub const MAX_MESSAGE_BYTES: usize = 48 * 1024;

pub const MAX_MESSAGES_PER_SECOND: u64 = 64;

pub const MAX_BYTES_PER_SECOND: u64 = 64 * 1024;

pub const MAX_QUEUE_MESSAGES: usize = 256;

pub const MAX_QUEUE_BYTES: usize = 256 * 1024;

pub const MAX_ACTIONS: usize = 32;

pub const MAX_CHANNELS: usize = 64;

pub const MAX_CHANNEL_FIELDS: usize = 64;

pub const MAX_FIELD_DEPTH: usize = 4;

pub const INITIAL_BUNDLE_GENERATION: u64 = 1;

pub const MAX_BUNDLES: usize = 4;

pub const MAX_COMPONENT_BYTES: usize = 16 * 1024 * 1024;

pub const MAX_BUNDLE_BYTES: usize = 64 * 1024 * 1024;

pub const MAX_EXPANDED_BYTES: u64 = 256 * 1024 * 1024;

pub const MAX_FILES: usize = 4096;

pub const MAX_GUEST_MEMORY: u64 = 32 * 1024 * 1024;

pub const MAX_SESSION_MEMORY: u64 = 64 * 1024 * 1024;

pub const MAX_GPU_BYTES: u64 = 128 * 1024 * 1024;

pub const MAX_HOST_OUTPUT: usize = MAX_MESSAGE_BYTES + 64 * 1024;

pub const MAX_WIDGET_TEXT_BYTES: usize = 512;

pub const MAX_DRAWS: u32 = 128;

pub const MAX_TRIANGLES: u32 = 100_000;

pub const MAX_PARTICLES: u32 = 4096;

pub const CACHE_QUOTA: u64 = 512 * 1024 * 1024;

pub const MAX_OFFER_LIFETIME_SECS: u64 = 24 * 60 * 60;

pub const NEGOTIATION_TIMEOUT_MS: u64 = 15_000;

pub const DEVELOPER_ENV: &str = "CINNABAR_DEV_SERVER_EXPERIENCES";

pub const MAX_GUEST_STRIKES: usize = 3;

pub const GUEST_STRIKE_WINDOW_MS: u64 = 60_000;

pub const MAX_TEMPLATES: usize = 32;

pub const MAX_TEMPLATE_BYTES: usize = 256 * 1024;

pub const MAX_TEXTURES: usize = 256;

pub const MAX_TEXTURE_BYTES: usize = 4 * 1024 * 1024;

pub const MAX_COLLECTIONS: usize = 32;

pub const MAX_COLLECTION_ROWS: usize = 4096;

pub const MAX_ROW_FIELDS: usize = 32;

pub const MAX_UI_VALUES: usize = 256;

pub const MAX_UI_NUMBERS: usize = 4;

pub const MAX_EDIT_TEXT_BYTES: usize = 512;
// END GENERATED EXTENSION CONTRACT

/// A local consent action; remote messages cannot create one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Once,
    Always,
    Never,
    Cancel,
    Disable,
}

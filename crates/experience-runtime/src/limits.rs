//! Every resource limit of the Experience runtime. This file is the only source of truth.

use std::time::Duration;

use crate::protocol::INVENTORY_SLOTS;

/// Fuel granted to one callback.
pub const CALLBACK_FUEL: u64 = 10_000_000;
/// Fuel granted to `register`.
pub const REGISTER_FUEL: u64 = 100_000_000;
/// Epoch tick period, the wall-clock backstop for fuel.
pub const EPOCH_PERIOD: Duration = Duration::from_millis(1);
/// Wall-clock deadline for one callback.
pub const CALLBACK_DEADLINE: Duration = Duration::from_millis(250);
/// Wall-clock deadline for `register`.
pub const REGISTER_DEADLINE: Duration = Duration::from_secs(2);
/// Linear memory per instance.
pub const MAX_MEMORY_BYTES: usize = 64 * 1024 * 1024;
/// Linear memories per instance.
pub const MAX_MEMORIES: usize = 1;
/// Table elements per instance.
pub const MAX_TABLE_ELEMENTS: usize = 10_000;
/// Core instances per component instance.
pub const MAX_CORE_INSTANCES: usize = 16;
/// Wasm stack.
pub const MAX_WASM_STACK_BYTES: usize = 256 * 1024;
/// Size of `server.wasm`.
pub const MAX_COMPONENT_BYTES: usize = 16 * 1024 * 1024;
/// Size of `experience.toml`.
pub const MAX_MANIFEST_BYTES: usize = 65_536;
/// Bytes in an Experience's version; at least one is required, and none may be a control
/// character.
pub const MAX_VERSION_BYTES: usize = 64;
/// Blocks one Experience may register.
pub const MAX_BLOCKS: usize = 64;
/// Bytes in a block's name, the part of its id after `<experience id>:`; at least one is
/// required.
pub const MAX_BLOCK_NAME_BYTES: usize = 32;
/// Bytes in a block's display name; at least one is required.
pub const MAX_DISPLAY_NAME_BYTES: usize = 64;
/// Bytes in a state's name after `<experience id>:`, a string state value, a material instance
/// and a geometry's name after `geometry.<experience id>.`; at least one is required.
pub const MAX_NAME_BYTES: usize = 64;
/// Values one string state may take (`Values.json`: 1 to 16).
pub const MAX_STATE_VALUES: usize = 16;
/// State combinations one block registers: its states' and placement traits' value counts
/// multiplied. The client's own bound per block, `MAX_STATES_PER_BLOCK` in
/// `crates/protocol/src/world/custom_blocks.rs`, must admit it, which a test there checks.
pub const MAX_STATE_COMBINATIONS: usize = 65_536;
/// Bones whose visibility one visual or permutation sets (`Detailed Geometry.json`).
pub const MAX_BONES: usize = 64;
/// Permutations one block declares.
pub const MAX_PERMUTATIONS: usize = 64;
/// Materials one visual or permutation lists.
pub const MAX_MATERIALS: usize = 32;
/// State tests in one condition, summed over its clauses.
pub const MAX_CONDITION_TESTS: usize = 64;
/// Frames one flipbook lists.
pub const MAX_FLIPBOOK_FRAMES: usize = 256;
/// Size of one geometry file.
pub const MAX_GEOMETRY_BYTES: usize = 1024 * 1024;
/// Host calls per callback; logs are counted separately. A callback on a network reads every
/// member, its id and then its states or data, and writes some of them, so the cap leaves room
/// for that at the network bounds.
pub const MAX_HOST_CALLS: usize = 4096;
/// Staged ops per callback. Rewriting a block's data replaces its staged op instead of adding
/// one.
pub const MAX_STAGED_OPS: usize = 64;
/// Bytes of block data one callback may stage, summed over its staged ops.
pub const MAX_STAGED_DATA_BYTES: usize = 262_144;
/// Tells per callback.
pub const MAX_TELLS: usize = 4;
/// UTF-8 bytes per tell.
pub const MAX_TELL_BYTES: usize = 256;
/// Client messages one callback may stage with `send-client`.
pub const MAX_CLIENT_SENDS: usize = 8;
/// Bytes of the client messages one callback may stage: their channels and their payloads as
/// JSON, summed. The adapter applies the client channel's own limits as it sends each one; this
/// leaves room for one message of the largest size that the client wire protocol carries, which
/// the Go adapter's tests check against the wire's constants.
pub const MAX_CLIENT_SEND_BYTES: usize = 65_536;
/// Lists and records nest at most this deep in a client message's payload, a top-level one being
/// level 1: the client wire protocol's `MAX_FIELD_DEPTH`, which the Go adapter's tests check.
pub const MAX_VALUE_DEPTH: usize = 4;
/// Bytes of data per block.
pub const MAX_BLOCK_DATA_BYTES: usize = 65_536;
/// UTF-8 bytes of a reason that reaches the adapter, a guest's error or why a load failed; the
/// rest is cut off at a char boundary.
pub const MAX_REASON_BYTES: usize = 512;
/// Logs per callback or `register`.
pub const MAX_LOGS: usize = 32;
/// Bytes per log line.
pub const MAX_LOG_BYTES: usize = 512;
/// Items one Experience may register.
pub const MAX_ITEMS: usize = 64;
/// The most one stack of an Experience's item may hold: a Bedrock stack's.
pub const MAX_STACK_SIZE: u8 = 64;
/// Bytes of an Experience's own data on one item stack.
pub const MAX_ITEM_DATA_BYTES: usize = 8192;
/// Items the adapter may list as the server's when it loads an Experience.
pub const MAX_SERVER_ITEMS: usize = 16_384;
/// Members one callback's network holds; the adapter's flood stops there and marks the network
/// truncated.
pub const MAX_NETWORK_BLOCKS: usize = 1024;
/// Bytes of block data one callback's network holds, summed over its members; the adapter's
/// flood stops before a member that would pass it and marks the network truncated.
pub const MAX_NETWORK_DATA_BYTES: usize = 524_288;
/// Encoded JSON bytes per IPC frame, excluding the 4-byte length prefix. The frame crosses a
/// local pipe between the adapter and the runtime, and a callback request carries a network and
/// an inventory: at the network bounds a request measured 2.37 MB, and an inventory whose every
/// slot holds MAX_ITEM_DATA_BYTES adds about 0.6 MB, 2.98 MB in all, which the Go adapter's
/// TestNetworkAtItsBoundsFitsAFrame pins. Truncating real AE2 networks at a smaller frame would
/// be worse.
pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

// Hex doubles staged data, which may fill at most half of a result frame; the other half is
// room for the remaining ops, client messages included.
const _: () = assert!(2 * MAX_STAGED_DATA_BYTES <= MAX_FRAME_BYTES / 2);
const _: () = assert!(MAX_CLIENT_SEND_BYTES <= MAX_FRAME_BYTES / 8);
// Hex doubles the network's data, which may fill at most half of a request frame; the other half
// is room for its cells, which the Go adapter's tests check at the bounds, and the rest.
const _: () = assert!(2 * MAX_NETWORK_DATA_BYTES <= MAX_FRAME_BYTES / 2);
// Hex doubles item data: a result's staged item data, one stack per op, fits beside its staged
// block data in half a result frame, and an inventory's in an eighth of a request frame.
const _: () = assert!(
    2 * (MAX_STAGED_DATA_BYTES + MAX_STAGED_OPS * MAX_ITEM_DATA_BYTES) <= MAX_FRAME_BYTES / 2
);
const _: () = assert!(2 * INVENTORY_SLOTS * MAX_ITEM_DATA_BYTES <= MAX_FRAME_BYTES / 4);
// A network-wide read: each member's id and its states or data, then the staged writes.
const _: () = assert!(2 * MAX_NETWORK_BLOCKS + MAX_STAGED_OPS <= MAX_HOST_CALLS);

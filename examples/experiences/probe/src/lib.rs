//! Test guest for the Experience runtime. `on-interact` selects a behavior by
//! `pos.x`; every other position it touches is relative to the interacted block
//! `p`, and `up` is the block above it. World errors are told by their WIT
//! kebab-case names. `client-message` reads the block of its player's focus and
//! writes its data, or without a focus tries the same at the origin, which its
//! callback refuses; it echoes the message back to the sender's client part and
//! tells what happened. `epoch` does the same read and write, resends a short
//! item list and tells what happened. Its channels come from its
//! `experience.toml`.

use experience_sdk::Value;
use experience_sdk::server::{
    BlockChange, BlockPos, Callback, Experience, Face, GuestError, LogLevel, PlayerId,
    Registration, WorldError, log, nodes,
};

/// The declarations of `experience.toml`, which `build.rs` writes.
mod experience {
    experience_sdk::include_declarations!();
}

use experience::channels;

mod contract;
mod interaction;
mod worlds;

const COUNTER: &str = "probe:counter";
const AIR: &str = "minecraft:air";
const NIL_PLAYER: &str = "00000000-0000-0000-0000-000000000000";

struct Probe;

impl Experience for Probe {
    /// Exercises ordered player observations through the canonical server world.
    fn on_player(
        ctx: &Callback,
        player: PlayerId,
        event: experience_sdk::server::PlayerEvent,
    ) -> Result<(), GuestError> {
        worlds::on_player(ctx, player, event)
    }

    /// Registers a destination definition for world API probes.
    fn world_definitions() -> Result<experience_sdk::server::WorldRegistration, GuestError> {
        Ok(worlds::definitions())
    }

    /// The 0.5 export: the counter, the lamp and the node.
    fn registration() -> Result<Registration, GuestError> {
        Ok(contract::registration(COUNTER))
    }

    fn on_place(ctx: &Callback, change: BlockChange) -> Result<(), GuestError> {
        tell_actor(ctx, &change, "placed");
        Ok(())
    }

    fn on_break(ctx: &Callback, change: BlockChange) -> Result<(), GuestError> {
        let len = change
            .previous_data
            .as_ref()
            .map_or_else(|| "none".to_owned(), |data| data.len().to_string());
        tell_actor(ctx, &change, &format!("broke {len}"));
        Ok(())
    }

    fn on_interact(
        ctx: &Callback,
        player: PlayerId,
        pos: BlockPos,
        _clicked_face: Face,
    ) -> Result<(), GuestError> {
        interaction::interact(ctx, &player, pos)
    }

    fn on_neighbor_changed(
        _ctx: &Callback,
        pos: BlockPos,
        neighbor: BlockPos,
    ) -> Result<(), GuestError> {
        log(
            LogLevel::Info,
            &format!("neighbor {neighbor:?} of {pos:?} changed"),
        );
        Ok(())
    }

    fn client_message(
        ctx: &Callback,
        player: PlayerId,
        channel: String,
        schema: u16,
        payload: Vec<Value>,
    ) -> Result<(), GuestError> {
        let access = world_access(ctx);
        let fields = payload.len();
        let echo = outcome(ctx.send_client(&player, &channel, schema, &nodes(payload)));
        let text = format!("client {channel} {schema} {fields} {access} echo {echo}");
        let _ = ctx.tell(&player, &text);
        Ok(())
    }

    /// Counts the update in the block's data, as x=0 counts interactions.
    fn on_scheduled_update(ctx: &Callback, pos: BlockPos) -> Result<(), GuestError> {
        log(LogLevel::Info, &format!("scheduled {}", count(ctx, pos)));
        Ok(())
    }

    fn epoch(ctx: &Callback, player: PlayerId) -> Result<(), GuestError> {
        let access = world_access(ctx);
        let channel = channels::ITEMS;
        let send = outcome(ctx.send_client(&player, channel.id, channel.schema, &nodes(items(2))));
        let _ = ctx.tell(&player, &format!("epoch {access} send {send}"));
        Ok(())
    }
}

experience_sdk::export_experience!(Probe);

/// The focus, and a block read and a data write at it, or at the origin without one, as
/// `focus {x y z|none} read {id|error} write {outcome}`; a callback without a snapshot refuses
/// both.
fn world_access(ctx: &Callback) -> String {
    let focus = ctx.focus();
    let target = focus.unwrap_or(BlockPos { x: 0, y: 64, z: 0 });
    let read = id_or_error(ctx.get_block(target));
    let write = outcome(ctx.set_block_data(target, Some(&[1])));
    format!("focus {} read {read} write {write}", describe(focus))
}

/// `x y z`, or `none`.
fn describe(pos: Option<BlockPos>) -> String {
    pos.map_or_else(
        || "none".to_owned(),
        |BlockPos { x, y, z }| format!("{x} {y} {z}"),
    )
}

/// An item list of `count` entries: one list of records, each an index and a name.
fn items(count: i64) -> Vec<Value> {
    let item = |i: i64| Value::Record(vec![Value::Integer(i), Value::Text(format!("item {i}"))]);
    vec![Value::List((0..count).map(item).collect())]
}

/// Describes [`next_count`]: `count {n}`, or `error {name}` if a call failed.
fn count(ctx: &Callback, p: BlockPos) -> String {
    match next_count(ctx, p) {
        Ok(n) => format!("count {n}"),
        Err(error) => format!("error {}", error.name()),
    }
}

/// Increments the little-endian u32 in `p`'s data (absent counts as 0) and
/// returns the new count.
fn next_count(ctx: &Callback, p: BlockPos) -> Result<u32, WorldError> {
    let next = ctx
        .block_data(p)?
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .map_or(0, u32::from_le_bytes)
        .wrapping_add(1);
    ctx.set_block_data(p, Some(&next.to_le_bytes()))?;
    Ok(next)
}

fn tell_actor(ctx: &Callback, change: &BlockChange, text: &str) {
    if let Some(actor) = &change.actor {
        let _ = ctx.tell(actor, text);
    }
}

fn id_or_error(result: Result<String, WorldError>) -> String {
    result.unwrap_or_else(|error| error.name().to_owned())
}

fn outcome(result: Result<(), WorldError>) -> &'static str {
    result.map_or_else(|error| error.name(), |()| "ok")
}

fn presence(result: Result<Option<Vec<u8>>, WorldError>) -> &'static str {
    match result {
        Ok(None) => "absent",
        Ok(Some(data)) if data.is_empty() => "empty",
        Ok(Some(_)) => "present",
        Err(error) => error.name(),
    }
}

//! Test guest for the Experience runtime. `on-interact` selects a behavior by
//! `pos.x`; every other position it touches is relative to the interacted block
//! `p`, and `up` is the block above it. World errors are told by their WIT
//! kebab-case names. `client-message` reads the block of its player's focus and
//! writes its data, or without a focus tries the same at the origin, which its
//! callback refuses; it echoes the message back to the sender's client part and
//! tells what happened. `epoch` does the same read and write, resends a short
//! item list and tells what happened. Its channels come from its
//! `experience.toml`.

use std::sync::atomic::{AtomicU32, Ordering};

use experience_sdk::Value;
use experience_sdk::server::{
    BlockChange, BlockDef, BlockPos, Callback, Experience, Face, GuestError, LogLevel, Mining,
    PlayerId, TextureBinding, WorldError, log, nodes,
};

/// The declarations of `experience.toml`, which `build.rs` writes.
mod experience {
    experience_sdk::include_declarations!();
}

use experience::channels;

const COUNTER: &str = "probe:counter";
const AIR: &str = "minecraft:air";
const NIL_PLAYER: &str = "00000000-0000-0000-0000-000000000000";
const MIB: usize = 1 << 20;

/// Bumped by x=12; a fresh instance per callback keeps it at 1.
static MEMORY: AtomicU32 = AtomicU32::new(0);

struct Probe;

impl Experience for Probe {
    fn register() -> Result<Vec<BlockDef>, GuestError> {
        Ok(vec![BlockDef {
            id: COUNTER.to_owned(),
            display_name: "Probe Counter".to_owned(),
            textures: vec![TextureBinding {
                slot: "*".to_owned(),
                path: "counter.png".to_owned(),
            }],
            mining: Mining::Breakable(1.0),
        }])
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
        interact(ctx, &player, pos)
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

    fn epoch(ctx: &Callback, player: PlayerId) -> Result<(), GuestError> {
        let access = world_access(ctx);
        let channel = channels::ITEMS;
        let send = outcome(ctx.send_client(&player, channel.id, channel.schema, &nodes(items(2))));
        let _ = ctx.tell(&player, &format!("epoch {access} send {send}"));
        Ok(())
    }
}

experience_sdk::export_experience!(Probe);

/// Runs the behavior that `p.x` selects.
fn interact(ctx: &Callback, player: &str, p: BlockPos) -> Result<(), GuestError> {
    let up = BlockPos { y: p.y + 1, ..p };
    let tell = |text: &str| {
        let _ = ctx.tell(player, text);
    };
    match p.x {
        0 => tell(&count(ctx, p)),
        1 => {
            let _ = ctx.set_block_data(p, Some(&[1]));
            tell("staged");
            let channel = channels::COUNTER;
            let record = nodes(vec![Value::Integer(1)]);
            let _ = ctx.send_client(player, channel.id, channel.schema, &record);
            // Lowers to the Wasm `unreachable` instruction.
            std::process::abort();
        }
        2 => loop {
            std::hint::spin_loop();
        },
        3 => {
            let mut hoard = Vec::<u8>::new();
            loop {
                hoard.resize(hoard.len() + MIB, 1);
                std::hint::black_box(&mut hoard);
            }
        }
        4 => {
            for _ in 0..300 {
                let _ = ctx.get_block(p);
            }
        }
        5 => {
            let _ = ctx.set_block(up, COUNTER);
            tell(&id_or_error(ctx.get_block(up)));
        }
        6 => tell(&id_or_error(ctx.get_block(BlockPos { x: p.x + 5, ..p }))),
        7 => tell(outcome(ctx.set_block_data(p, Some(&vec![0; 65_537])))),
        8 => tell(outcome(ctx.set_block(p, "minecraft:stone"))),
        9 => tell(outcome(ctx.tell(NIL_PLAYER, "x"))),
        10 => {
            tell("staged");
            return Err(GuestError::Rejected("nope".to_owned()));
        }
        11 => {
            for line in 0..1000 {
                log(LogLevel::Debug, &format!("flood {line}"));
            }
            tell("logged");
        }
        12 => {
            let n = MEMORY.fetch_add(1, Ordering::Relaxed) + 1;
            tell(&format!("mem {n}"));
        }
        13 => {
            let _ = ctx.set_block_data(p, None);
            tell(presence(ctx.block_data(p)));
        }
        14 => {
            let _ = ctx.set_block_data(p, Some(&[]));
            tell(presence(ctx.block_data(p)));
        }
        15 => {
            let info = ctx.info();
            tell(&format!("tick {} seq {}", info.tick, info.event_sequence));
        }
        16 => {
            for step in 0..65 {
                let _ = ctx.set_block(up, if step % 2 == 0 { AIR } else { COUNTER });
            }
        }
        17 => {
            let _ = ctx.set_block(up, COUNTER);
            match ctx.block_data(up) {
                Err(error) => tell(&format!("error {}", error.name())),
                data => tell(presence(data)),
            }
        }
        // About 2 MiB of the 3-byte `€`, so a byte limit can fall inside a character.
        18 => return Err(GuestError::Rejected("€".repeat(2 * MIB / 3))),
        19 => match next_count(ctx, p) {
            Ok(n) => {
                let channel = channels::COUNTER;
                let record = nodes(vec![Value::Integer(n.into())]);
                let sent = outcome(ctx.send_client(player, channel.id, channel.schema, &record));
                tell(&format!("count {n} {sent}"));
            }
            Err(error) => tell(&format!("error {}", error.name())),
        },
        20 => tell(outcome(ctx.send_client(
            NIL_PLAYER,
            channels::COUNTER.id,
            channels::COUNTER.schema,
            &[],
        ))),
        21 => {
            let text = Value::Text("x".repeat(MIB));
            tell(outcome(ctx.send_client(
                player,
                channels::COUNTER.id,
                channels::COUNTER.schema,
                &nodes(vec![text]),
            )));
        }
        22 => {
            let channel = channels::ITEMS;
            let record = nodes(items(400));
            let sent = outcome(ctx.send_client(player, channel.id, channel.schema, &record));
            tell(&format!("items {sent}"));
        }
        23 => tell(&format!("focus {}", describe(ctx.focus()))),
        x => return Err(GuestError::Rejected(format!("no probe behavior for x={x}"))),
    }
    Ok(())
}

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

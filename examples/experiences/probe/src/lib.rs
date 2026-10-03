//! Test guest for the Experience runtime. `on-interact` selects a behavior by
//! `pos.x`; every other position it touches is relative to the interacted block
//! `p`, and `up` is the block above it. World errors are told by their WIT
//! kebab-case names. `client-message` tries a block read and write, which its
//! callback refuses, echoes the message back to the sender's client part and
//! tells what happened.

use std::sync::atomic::{AtomicU32, Ordering};

use experience_sdk::{
    BlockChange, BlockDef, BlockPos, Callback, Experience, Face, GuestError, LogLevel, Mining,
    PlayerId, Scalar, TextureBinding, WorldError, log,
};

const COUNTER: &str = "probe:counter";
/// The client channel that x=19 sends the counter on.
const COUNTER_CHANNEL: &str = "probe.counter";
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
        payload: Vec<Scalar>,
    ) -> Result<(), GuestError> {
        let origin = BlockPos { x: 0, y: 64, z: 0 };
        let read = id_or_error(ctx.get_block(origin));
        let write = outcome(ctx.set_block(origin, COUNTER));
        let echo = outcome(ctx.send_client(&player, &channel, schema, &payload));
        let fields = payload.len();
        let text =
            format!("client {channel} {schema} {fields} read {read} write {write} echo {echo}");
        let _ = ctx.tell(&player, &text);
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
            let _ = ctx.send_client(player, COUNTER_CHANNEL, 1, &[Scalar::Integer(1)]);
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
                let value = Scalar::Integer(n.into());
                let sent = outcome(ctx.send_client(player, COUNTER_CHANNEL, 1, &[value]));
                tell(&format!("count {n} {sent}"));
            }
            Err(error) => tell(&format!("error {}", error.name())),
        },
        20 => tell(outcome(ctx.send_client(
            NIL_PLAYER,
            COUNTER_CHANNEL,
            1,
            &[],
        ))),
        21 => {
            let text = Scalar::Text("x".repeat(MIB));
            tell(outcome(ctx.send_client(
                player,
                COUNTER_CHANNEL,
                1,
                &[text],
            )));
        }
        x => return Err(GuestError::Rejected(format!("no probe behavior for x={x}"))),
    }
    Ok(())
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

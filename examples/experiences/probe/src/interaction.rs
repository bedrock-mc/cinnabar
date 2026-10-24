//! Interaction-selected runtime probes.

use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

const MIB: usize = 1 << 20;

/// Bumped by x=12; a fresh instance per callback keeps it at 1.
static MEMORY: AtomicU32 = AtomicU32::new(0);

/// Runs the behavior that `p.x` selects.
pub(super) fn interact(ctx: &Callback, player: &str, p: BlockPos) -> Result<(), GuestError> {
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
        // More calls than any host call cap.
        4 => {
            for _ in 0..=u16::MAX {
                let _ = ctx.get_block(p);
            }
        }
        5 => {
            let _ = ctx.set_block(up, COUNTER);
            tell(&id_or_error(ctx.get_block(up)));
        }
        6 => tell(&id_or_error(ctx.get_block(BlockPos { x: p.x + 5, ..p }))),
        7 => tell(outcome(ctx.set_block_data(p, Some(&vec![0; 65_537])))),
        8 => tell(outcome(ctx.set_block(p, UNOWNED_BLOCK))),
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
        24 => tell(&contract::wit_0_5_calls(ctx, p, up)),
        25 => tell(&contract::toggle_lamp(ctx, p)),
        26 => tell(&contract::mark_network(ctx)),
        27 => tell(&contract::make_stacks(ctx, up)),
        28 => tell(&contract::craft(ctx)),
        29 => tell(&contract::schedule(ctx, p)),
        30..=33 => return worlds::interact(ctx, player, p.x),
        x => return Err(GuestError::Rejected(format!("no probe behavior for x={x}"))),
    }
    Ok(())
}

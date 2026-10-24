use std::sync::Arc;

use protocol::{
    BlockCrackAction, BlockCrackEvent, HudEvent, UiEvent, WorldEvent, decode_batch,
    into_world_event,
};
use ui::UiAction;

use super::*;

#[path = "../../tests/support/ui_events.rs"]
#[allow(
    dead_code,
    reason = "shared with the integration target, which uses more fixtures"
)]
mod fixtures;
use fixtures::{envelope, raw_text_event};

#[test]
fn block_cracks_are_consumed_in_sequence_and_cleared_on_session_change() {
    let mut player_runtime = player_state::PlayerState::new(4);

    let mut runtime = UiRuntime::new(4);
    let event = BlockCrackEvent {
        position: [3, 64, -2],
        action: BlockCrackAction::Start {
            progress_per_tick: 1_024,
        },
    };

    runtime
        .retain_block_crack(SequencedBlockCrackEvent {
            session_id: 4,
            fifo_sequence: 7,
            dimension: 0,
            event,
        })
        .unwrap();

    // Envelopes validate ordering only; active authority arrives from the stream.
    assert_eq!(runtime.block_cracks.status().active, 0);
    assert_eq!(runtime.block_cracks.status().consumed, 0);
    assert!(matches!(
        runtime.retain_block_crack(SequencedBlockCrackEvent {
            session_id: 4,
            fifo_sequence: 7,
            dimension: 0,
            event,
        }),
        Err(UiRuntimeError::StaleBlockCrackSequence { .. })
    ));

    player_runtime.begin_session(5);
    runtime.begin_session(5);
    assert_eq!(runtime.block_cracks.status().active, 0);
    assert_eq!(runtime.block_cracks.status().consumed, 0);
}

#[test]
fn block_crack_stops_consume_without_retaining_a_history() {
    let mut runtime = UiRuntime::new(9);
    for sequence in 0..1_280_u64 {
        runtime
            .retain_block_crack(SequencedBlockCrackEvent {
                session_id: 9,
                fifo_sequence: sequence,
                dimension: 1,
                event: BlockCrackEvent {
                    position: [sequence as i32, 0, 0],
                    action: BlockCrackAction::Stop,
                },
            })
            .unwrap();
    }

    assert_eq!(runtime.block_cracks.status().active, 0);
    assert_eq!(runtime.block_cracks.status().consumed, 0);
}

mod chat_tests;
mod container_address_tests;
mod data_driven_tests;
mod death_tests;
mod furnace_options_tests;
mod gameplay_hud_tests;
mod inventory_open_tests;
mod inventory_options_tests;
mod inventory_overlay_tests;
mod inventory_timeout_tests;
mod localized_text_tests;
mod quick_craft_tests;
mod retained_bounds_tests;
mod trading_tests;

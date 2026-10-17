//! Server WIT 0.6 on the probe: the server's crafting recipes (x=28) and scheduled updates
//! (x=29, and the update itself).

use crate::common;

use common::{outcome, p, scheduled, tell};
use experience_runtime::protocol::{Op, Outcome};

#[test]
fn the_servers_recipes_answer_a_grid_and_refuse_a_short_one() {
    assert_eq!(
        outcome(&common::interact(28)),
        Outcome::Committed {
            ops: vec![tell("crafted minecraft:stick 4 short none")]
        }
    );
}

#[test]
fn an_owned_block_takes_an_update_but_zero_ticks_and_foreign_blocks_do_not() {
    assert_eq!(
        outcome(&common::interact(29)),
        Outcome::Committed {
            ops: vec![
                Op::ScheduleUpdate {
                    pos: p(29),
                    ticks: 2,
                },
                tell("schedule ok zero denied foreign not-owned"),
            ]
        }
    );
}

#[test]
fn a_due_update_runs_with_the_blocks_snapshot() {
    assert_eq!(
        outcome(&scheduled(p(0))),
        Outcome::Committed {
            ops: vec![Op::SetBlockData {
                pos: p(0),
                data: Some("01000000".to_owned()),
            }]
        }
    );
}

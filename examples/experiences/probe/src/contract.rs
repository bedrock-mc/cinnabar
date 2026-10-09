//! Server WIT 0.5's calls. x=24 makes each once; x=25 toggles a lamp; x=26 marks every member of
//! a node's network; x=27 makes stacks.

use experience_sdk::server::{
    BlockDef, BlockPos, BlockState, BlockType, Callback, ItemDef, Mining, NewStack,
    PlacementStates, Registration, StateDef, StateValue, StateValues, TextureBinding, WorldError,
    cube,
};

/// A cube with the facing placement trait and one bool state, [`LAMP_ON`].
const LAMP: &str = "probe:lamp";
/// A cube that is a network member.
const NODE: &str = "probe:node";
/// The probe's item, one to a stack.
const CELL: &str = "probe:cell";
pub(crate) const LAMP_ON: &str = "probe:on";

/// The probe's blocks, each a cube showing `counter.png`: the counter `counter`, without states,
/// the lamp, with the facing trait and [`LAMP_ON`], and the node, a network member; and the
/// item [`CELL`], one to a stack, with `counter.png` for an icon.
pub(crate) fn registration(counter: &str) -> Registration {
    let def = |id: &str, display_name: &str| BlockDef {
        id: id.to_owned(),
        display_name: display_name.to_owned(),
        textures: vec![TextureBinding {
            slot: "*".to_owned(),
            path: "counter.png".to_owned(),
        }],
        mining: Mining::Breakable(1.0),
    };
    let lamp = BlockType {
        states: vec![StateDef {
            name: LAMP_ON.to_owned(),
            values: StateValues::Bool,
        }],
        placement: PlacementStates::FACING_DIRECTION,
        ..cube(def(LAMP, "Probe Lamp"))
    };
    Registration {
        blocks: vec![
            cube(def(counter, "Probe Counter")),
            lamp,
            BlockType {
                network: true,
                ..cube(def(NODE, "Probe Node"))
            },
        ],
        items: vec![ItemDef {
            id: CELL.to_owned(),
            display_name: "Probe Cell".to_owned(),
            icon: "counter.png".to_owned(),
            max_stack: 1,
        }],
    }
}

/// Places a lamp at `up`, the block above the interacted block `p`, reads its states, lights it
/// and reads them again, then makes each other 0.5 call once, as `<call> <states|ok|error>`
/// pairs short enough for one tell.
pub(crate) fn wit_0_5_calls(ctx: &Callback, p: BlockPos, up: BlockPos) -> String {
    let stack = || NewStack {
        id: "minecraft:stone".to_owned(),
        metadata: 0,
        count: 1,
        data: None,
    };
    // Its outcome shows in the states read after it.
    let _ = ctx.set_block(up, LAMP);
    let before = states(ctx.block_states(up));
    let lit = refusal(ctx.set_block_state(up, &[on(true)]));
    let after = states(ctx.block_states(up));
    format!(
        "states {before} set {lit} states {after} network {} inventory {} set-slot {} \
         drop-item {}",
        refusal(ctx.network()),
        refusal(ctx.inventory()),
        refusal(ctx.set_slot(0, Some(&stack()))),
        refusal(ctx.drop_item(p, &stack())),
    )
}

/// Turns the lamp at `p` on or off, keeping its data, and tells its states.
pub(crate) fn toggle_lamp(ctx: &Callback, p: BlockPos) -> String {
    let lit = ctx.block_states(p).map(|states| {
        states
            .iter()
            .any(|state| state.name == LAMP_ON && matches!(state.value, StateValue::Bool(true)))
    });
    match lit {
        Ok(lit) => match ctx.set_block_state(p, &[on(!lit)]) {
            Ok(()) => format!("lamp {}", states(ctx.block_states(p))),
            Err(error) => format!("error {}", error.name()),
        },
        Err(error) => format!("error {}", error.name()),
    }
}

/// Writes, to every member of the anchor's network, the member count as one byte of data, and
/// tells `network {members} truncated {bool} wrote {writes}`, or `network none` off a network.
pub(crate) fn mark_network(ctx: &Callback) -> String {
    match ctx.network() {
        Ok(Some(network)) => {
            let members = network.blocks.len();
            let mark = [members as u8];
            let wrote = network
                .blocks
                .iter()
                .filter(|pos| ctx.set_block_data(**pos, Some(&mark)).is_ok())
                .count();
            format!(
                "network {members} truncated {} wrote {wrote}",
                network.truncated
            )
        }
        Ok(None) => "network none".to_owned(),
        Err(error) => format!("error {}", error.name()),
    }
}

/// Reads the held stack, puts a cell with data `07` in slot 0 and drops two stone at `up`, and
/// tells `held {id} {count}/{max} plain {bool}|held none|error {name} cell {outcome} drop
/// {outcome}`.
pub(crate) fn make_stacks(ctx: &Callback, up: BlockPos) -> String {
    let held = match ctx.inventory() {
        Ok(inventory) => match &inventory.slots[usize::from(inventory.selected)] {
            Some(stack) => format!(
                "{} {}/{} plain {}",
                stack.id, stack.count, stack.max_count, stack.plain
            ),
            None => "none".to_owned(),
        },
        Err(error) => format!("error {}", error.name()),
    };
    let stack = |id: &str, count: u8, data: Option<Vec<u8>>| NewStack {
        id: id.to_owned(),
        metadata: 0,
        count,
        data,
    };
    let cell = refusal(ctx.set_slot(0, Some(&stack(CELL, 1, Some(vec![7])))));
    let drop = refusal(ctx.drop_item(up, &stack("minecraft:stone", 2, None)));
    format!("held {held} cell {cell} drop {drop}")
}

fn on(lit: bool) -> BlockState {
    BlockState {
        name: LAMP_ON.to_owned(),
        value: StateValue::Bool(lit),
    }
}

/// States as `name=value` joined by `,`, or the error's name.
fn states(result: Result<Vec<BlockState>, WorldError>) -> String {
    match result {
        Ok(states) => states
            .iter()
            .map(|state| match &state.value {
                StateValue::Bool(value) => format!("{}={value}", state.name),
                StateValue::Choice(value) => format!("{}={value}", state.name),
            })
            .collect::<Vec<_>>()
            .join(","),
        Err(error) => error.name().to_owned(),
    }
}

/// A world error's name, or `ok` for any success.
fn refusal<T>(result: Result<T, WorldError>) -> &'static str {
    result.map_or_else(|error| error.name(), |_| "ok")
}

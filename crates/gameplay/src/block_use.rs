//! Block-use classification, held-repeat state and ordered packet production.
use crate::{interaction_authority::FrozenBlockObservation, movement::PhysicsCollisionRegistries};
use client_world::game_mode_capabilities::GameModeCapabilities;
use protocol::{BlockUseRequest, ItemUseTrigger, SwingSource, VerifiedNetworkItemStack};

// Held repeats are checked on simulation ticks against a monotonic deadline.
const SLOW_REPEAT_MILLIS: u64 = 300;
const STILL_REPEAT_MILLIS: u64 = 200;
const MOVING_REPEAT_MAX_MILLIS: u64 = 180;
/// Moving repeats wait this many milliseconds per block/second of speed.
const MOVING_REPEAT_BLOCK_MILLIS: f32 = 900.0;
const SURVIVAL_REPEAT_FLOOR_MILLIS: u64 = 100;
/// How far behind now a moving repeat's schedule may lag.
const REPEAT_MAX_LAG_MILLIS: u64 = 180;
/// Placement boxes shrink by this much per side before the actor test.
const PLACEMENT_ACTOR_EPSILON: f64 = 1.0e-5;

/// Blocks a placement replaces in place. Provisional list; needs independent measurement.
const REPLACEABLE_BLOCKS: &[&str] = &[
    "minecraft:air",
    "minecraft:short_grass",
    "minecraft:tall_grass",
    "minecraft:fern",
    "minecraft:large_fern",
    "minecraft:deadbush",
    "minecraft:vine",
    "minecraft:glow_lichen",
    "minecraft:seagrass",
    "minecraft:water",
    "minecraft:flowing_water",
    "minecraft:lava",
    "minecraft:flowing_lava",
    "minecraft:fire",
    "minecraft:soul_fire",
    "minecraft:structure_void",
    "minecraft:crimson_roots",
    "minecraft:warped_roots",
    "minecraft:nether_sprouts",
    "minecraft:hanging_roots",
];

/// Which ability a block's own use needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Interaction {
    /// Doors, trapdoors, fence gates, buttons and levers.
    Switch,
    Container,
    /// Uses no ability gates.
    Other,
}

/// Container-screen blocks. Provisional list; needs independent measurement.
const CONTAINER_BLOCKS: &[&str] = &[
    "minecraft:crafting_table",
    "minecraft:furnace",
    "minecraft:lit_furnace",
    "minecraft:blast_furnace",
    "minecraft:lit_blast_furnace",
    "minecraft:smoker",
    "minecraft:lit_smoker",
    "minecraft:barrel",
    "minecraft:anvil",
    "minecraft:enchanting_table",
    "minecraft:brewing_stand",
    "minecraft:hopper",
    "minecraft:dropper",
    "minecraft:dispenser",
    "minecraft:crafter",
    "minecraft:loom",
    "minecraft:stonecutter_block",
    "minecraft:grindstone",
    "minecraft:cartography_table",
    "minecraft:smithing_table",
    "minecraft:beacon",
];

/// Other blocks whose own use succeeds locally. Provisional list; needs independent measurement.
const OTHER_INTERACTIVE_BLOCKS: &[&str] = &[
    "minecraft:noteblock",
    "minecraft:unpowered_repeater",
    "minecraft:powered_repeater",
    "minecraft:unpowered_comparator",
    "minecraft:powered_comparator",
    "minecraft:daylight_detector",
    "minecraft:daylight_detector_inverted",
    "minecraft:bell",
    "minecraft:bed",
];

fn interaction(identifier: &str) -> Option<Interaction> {
    let metal = identifier == "minecraft:iron_door" || identifier == "minecraft:iron_trapdoor";
    let ends = |suffixes: &[&str]| suffixes.iter().any(|suffix| identifier.ends_with(suffix));
    if identifier == "minecraft:lever"
        || (!metal && ends(&["_door", "_trapdoor", "_button", "fence_gate"]))
    {
        Some(Interaction::Switch)
    } else if CONTAINER_BLOCKS.contains(&identifier) || ends(&["chest", "shulker_box"]) {
        Some(Interaction::Container)
    } else if OTHER_INTERACTIVE_BLOCKS.contains(&identifier) || identifier.ends_with("_bed") {
        Some(Interaction::Other)
    } else {
        None
    }
}

impl Interaction {
    const fn permitted(self, caps: &GameModeCapabilities) -> bool {
        match self {
            Self::Switch => caps.can_use_switches,
            Self::Container => caps.can_open_containers,
            Self::Other => true,
        }
    }
}

/// Milliseconds until the next held-use repeat.
///
/// `slow` covers an interactive use and a placement before its line is established.
pub fn repeat_interval_millis(sneaking: bool, slow: bool, speed: f32, survival: bool) -> u64 {
    let interval = if sneaking || slow {
        SLOW_REPEAT_MILLIS
    } else if speed.is_finite() && speed > 0.0 {
        ((MOVING_REPEAT_BLOCK_MILLIS / speed) as u64).min(MOVING_REPEAT_MAX_MILLIS)
    } else {
        STILL_REPEAT_MILLIS
    };
    if survival {
        interval.max(SURVIVAL_REPEAT_FLOOR_MILLIS)
    } else {
        interval
    }
}

/// The cell a block placed against `face` of `clicked` would occupy.
pub const fn placement_cell(clicked: [i32; 3], face: u8) -> [i32; 3] {
    let [x, y, z] = clicked;
    match face {
        0 => [x, y - 1, z],
        1 => [x, y + 1, z],
        2 => [x, y, z - 1],
        3 => [x, y, z + 1],
        4 => [x - 1, y, z],
        _ => [x + 1, y, z],
    }
}

/// A feet-anchored box, as `(min, max)`.
pub type BoxBounds = ([f64; 3], [f64; 3]);

/// Whether a block-local box placed in `cell` overlaps an actor box.
fn overlaps(cell: [i32; 3], (local_min, local_max): BoxBounds, (min, max): BoxBounds) -> bool {
    (0..3).all(|axis| {
        let low = f64::from(cell[axis]) + local_min[axis] + PLACEMENT_ACTOR_EPSILON;
        let high = f64::from(cell[axis]) + local_max[axis] - PLACEMENT_ACTOR_EPSILON;
        low < max[axis] && min[axis] < high
    })
}

const FULL_CELL: BoxBounds = ([0.0; 3], [1.0; 3]);

/// World facts one local use depends on.
#[derive(Debug, Clone, PartialEq)]
pub struct UseSurroundings {
    pub clicked_identifier: Option<String>,
    /// Canonical block state used to classify state-dependent block interactions.
    pub clicked_canonical_state: Option<String>,
    /// The selected block item's resolved block type, including server remaps.
    pub held_block_identifier: Option<String>,
    /// Identifier of the cell across the clicked face; `None` when unreadable.
    pub neighbor_identifier: Option<String>,
    pub player_box: BoxBounds,
    /// Boxes of actors that obstruct placement.
    pub actor_boxes: Vec<BoxBounds>,
    pub sneaking: bool,
    /// Block-local collision boxes of the held block; `None` when unknown, which
    /// tests the whole cell.
    pub placed_boxes: Option<Vec<BoxBounds>>,
}

impl UseSurroundings {
    /// The cell a placement fills: the clicked block when it is replaceable,
    /// otherwise the neighbor across the clicked face.
    pub fn destination(&self, clicked: [i32; 3], face: u8) -> ([i32; 3], bool) {
        let replaceable = |identifier: Option<&str>| {
            identifier.is_some_and(|identifier| REPLACEABLE_BLOCKS.contains(&identifier))
        };
        if replaceable(self.clicked_identifier.as_deref()) {
            (clicked, true)
        } else {
            let cell = placement_cell(clicked, face);
            (cell, replaceable(self.neighbor_identifier.as_deref()))
        }
    }
}

/// The local outcome of one click, which sets the prediction flag and swing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalUse {
    Interact,
    Place,
    Nothing,
}

impl LocalUse {
    /// A block use the capabilities deny falls through to item use, as vanilla's does.
    pub fn resolve(
        item: &VerifiedNetworkItemStack,
        clicked: [i32; 3],
        face: u8,
        surroundings: &UseSurroundings,
        caps: &GameModeCapabilities,
    ) -> Self {
        let clicked_identifier = surroundings.clicked_identifier.as_deref();
        let holding = item.network_id() != 0 && item.count() > 0;
        // Sneaking with an item uses the item instead of the block.
        let block_interaction = if clicked_identifier == Some("minecraft:respawn_anchor") {
            respawn_anchor::uses_block(surroundings).then_some(Interaction::Other)
        } else {
            clicked_identifier.and_then(interaction)
        };
        if block_interaction.is_some_and(|interaction| interaction.permitted(caps))
            && !(surroundings.sneaking && holding)
        {
            return Self::Interact;
        }
        if !caps.can_build || item.block_runtime_id() == 0 || item.count() == 0 {
            return Self::Nothing;
        }
        let (cell, free) = surroundings.destination(clicked, face);
        let placed = surroundings
            .placed_boxes
            .as_deref()
            .unwrap_or(std::slice::from_ref(&FULL_CELL));
        let blocked = std::iter::once(&surroundings.player_box)
            .chain(&surroundings.actor_boxes)
            .any(|bounds| placed.iter().any(|local| overlaps(cell, *local, *bounds)));
        if free && !blocked {
            Self::Place
        } else {
            Self::Nothing
        }
    }

    const fn swing(self) -> Option<SwingSource> {
        match self {
            Self::Interact => Some(SwingSource::Interact),
            Self::Place => Some(SwingSource::Build),
            Self::Nothing => None,
        }
    }
}

/// Press latch and the held-repeat schedule.
#[derive(Debug, Default)]
pub struct BlockUseRuntime {
    stopping: bool,
    stop_repress: bool,
    pub inventory: HeldPlacementInventory,
    latched_press: bool,
    waiting_for_authority: bool,
    last_use_millis: Option<u64>,
    /// The last success was a block interaction.
    slow_repeat: bool,
    last_attempt_tick: Option<u64>,
    /// Tick whose block interaction consumes the item-use press.
    interacted_tick: Option<u64>,
    /// The latest press was resolved as a block interaction.
    press_interacted: bool,
    position_authority: Option<(u64, u64)>,
    pub intention: BuildIntention,
    selected_item: Option<(u8, i32, i32)>,
    rejected_tick: Option<u64>,
}

/// One held-use repeat's timing inputs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RepeatClock {
    pub now_millis: u64,
    pub sneaking: bool,
    /// Full 3D speed in blocks per second.
    pub speed: f32,
    pub survival: bool,
}

impl RepeatClock {
    /// Noncreative modes share the survival repeat floor and pick reach.
    pub fn for_game_mode(
        now_millis: u64,
        sneaking: bool,
        speed: f32,
        game_mode: Option<protocol::PlayerGameMode>,
    ) -> Self {
        Self {
            now_millis,
            sneaking,
            speed,
            survival: game_mode != Some(protocol::PlayerGameMode::Creative),
        }
    }

    /// Times a build action from the tick-end state it observes.
    pub fn for_state(
        now_millis: u64,
        state: &crate::movement::UnsentSampleView,
        game_mode: Option<protocol::PlayerGameMode>,
    ) -> Self {
        let speed = state
            .displacement
            .map(|axis| axis * sim::TICKS_PER_SECOND as f32)
            .into_iter()
            .map(|axis| axis * axis)
            .sum::<f32>()
            .sqrt();
        Self::for_game_mode(now_millis, state.sneaking, speed, game_mode)
    }
}

impl BlockUseRuntime {
    /// Returns the successful destination retained until the held action stops.
    pub fn last_success_destination(&self) -> Option<[i32; 3]> {
        self.intention.last_success_destination()
    }

    /// A slot or item-type change stops the old placement line without inventing a press.
    pub fn selection_changed(&mut self, selection: &crate::mining::FrozenMiningSelection) -> bool {
        let identity = (
            selection.slot,
            selection.item.network_id(),
            selection.item.block_runtime_id(),
        );
        let changed = self
            .selected_item
            .is_some_and(|previous| previous != identity);
        self.selected_item = Some(identity);
        if changed {
            self.rejected_tick = None;
        }
        self.stop_repress |= changed && self.latched_press;
        changed
    }

    /// Cancels pending presses without dropping the repeat schedule or a stop destination.
    pub fn clear_press(&mut self) {
        self.rejected_tick = None;
        self.latched_press = false;
        self.stop_repress = false;
    }

    pub fn clear(&mut self) {
        self.rejected_tick = None;
        self.stopping = false;
        self.stop_repress = false;
        self.latched_press = false;
        self.slow_repeat = false;
        self.intention = BuildIntention::default();
    }

    /// Latches eligible input and suspends attempts until movement authority is ready.
    pub fn observe_use(
        &mut self,
        held: bool,
        pressed: bool,
        attacking: bool,
        authority_ready: bool,
    ) -> bool {
        self.waiting_for_authority = !authority_ready;
        if attacking || !(held || pressed || self.latched_press) {
            self.clear();
            return false;
        }
        if pressed {
            self.clear();
        }
        self.latched_press |= pressed;
        true
    }

    /// Commits the use schedule after transport admission, retaining refused presses for retry.
    pub fn admit(
        &mut self,
        trigger: ItemUseTrigger,
        due: u64,
        tick: u64,
        local_use: LocalUse,
        clock: RepeatClock,
        admitted: bool,
    ) -> bool {
        if admitted {
            self.record(trigger, due, tick, local_use, clock);
        } else if trigger == ItemUseTrigger::PlayerInput {
            self.latched_press = true;
        }
        admitted
    }

    /// Position corrections revoke pending presses while retaining admitted holds and inventory.
    pub fn synchronize(&mut self, authority: (u64, u64)) {
        if let Some(previous) = self.position_authority
            && previous != authority
        {
            self.rejected_tick = None;
            if previous.0 == authority.0 {
                self.latched_press = false;
            } else {
                *self = Self::default();
            }
        }
        self.position_authority = Some(authority);
    }

    /// The trigger due now, with the repeat's due time; at most one attempt per tick.
    pub fn due(&self, held: bool, tick: u64, clock: RepeatClock) -> Option<(ItemUseTrigger, u64)> {
        if self.waiting_for_authority {
            return None;
        }
        if self.latched_press {
            return Some((ItemUseTrigger::PlayerInput, clock.now_millis));
        }
        if !held || self.last_attempt_tick == Some(tick) {
            return None;
        }
        let interval = repeat_interval_millis(
            clock.sneaking,
            self.slow_repeat || self.intention.unlined(),
            clock.speed,
            clock.survival,
        );
        let due = self
            .last_use_millis
            .map_or(0, |last| last.saturating_add(interval));
        (clock.now_millis > due).then_some((ItemUseTrigger::SimulationTick, due))
    }

    /// Whether the use press resolved on `tick` interacted with a block.
    pub fn interacted_at(&self, tick: u64) -> bool {
        self.interacted_tick == Some(tick)
    }

    /// A press block use has latched but not yet resolved; item use waits for it.
    pub const fn press_pending(&self) -> bool {
        self.latched_press
    }

    /// Whether the latest press was resolved as a block interaction, consuming it.
    pub const fn press_interacted(&self) -> bool {
        self.press_interacted
    }

    /// A new press edge starts without the previous press's resolution.
    pub fn forget_press_resolution(&mut self) {
        self.press_interacted = false;
    }

    /// Records an attempt. As in vanilla, a failed repeat keeps its schedule, so it
    /// retries (and resends its transaction) on the next tick.
    pub fn record(
        &mut self,
        trigger: ItemUseTrigger,
        due: u64,
        tick: u64,
        local_use: LocalUse,
        clock: RepeatClock,
    ) {
        self.rejected_tick = None;
        self.latched_press = false;
        self.last_attempt_tick = Some(tick);
        if trigger == ItemUseTrigger::PlayerInput {
            self.press_interacted = local_use == LocalUse::Interact;
            if self.press_interacted {
                self.interacted_tick = Some(tick);
            }
        }
        if local_use == LocalUse::Nothing {
            return;
        }
        let moving = clock.speed.is_finite() && clock.speed > 0.0;
        self.last_use_millis = Some(match trigger {
            ItemUseTrigger::SimulationTick if moving => {
                due.max(clock.now_millis.saturating_sub(REPEAT_MAX_LAG_MILLIS))
            }
            ItemUseTrigger::SimulationTick | ItemUseTrigger::PlayerInput => clock.now_millis,
        });
        self.slow_repeat = local_use == LocalUse::Interact;
    }
}

/// The state a switch use predicts for the clicked block.
pub fn predicted_toggle(
    collisions: &PhysicsCollisionRegistries,
    stream: &impl crate::GameplayWorld,
    clicked: u32,
) -> Option<u32> {
    let mode = stream.network_id_mode();
    let identifier = collisions.block_identifier(mode, clicked)?;
    let states = toggled_states(identifier, collisions.block_canonical_state(mode, clicked)?)?;
    collisions.block_state_runtime_id(mode, identifier, &states)
}

/// Trapdoors and levers flip `open_bit`; an
/// unpressed button presses. Doors and fence gates also change their other
/// half or facing, which is not modelled, so they wait for the server.
pub fn toggled_states(
    identifier: &str,
    canonical_state: &str,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let iron = identifier == "minecraft:iron_trapdoor";
    let (property, press_only) =
        if identifier == "minecraft:lever" || (!iron && identifier.ends_with("_trapdoor")) {
            ("open_bit", false)
        } else if identifier.ends_with("_button") {
            ("button_pressed_bit", true)
        } else {
            return None;
        };
    let mut states =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(canonical_state).ok()?;
    let entry = states.get_mut(property)?;
    // Typed values carry the bit in `value`.
    let bit = match entry {
        serde_json::Value::Object(typed) => typed.get_mut("value")?,
        plain => plain,
    };
    let set = match bit {
        serde_json::Value::Bool(value) => *value,
        serde_json::Value::Number(value) => value.as_u64()? != 0,
        _ => return None,
    };
    if press_only && set {
        return None;
    }
    *bit = match bit {
        serde_json::Value::Bool(_) => serde_json::Value::Bool(!set),
        _ => serde_json::Value::from(u8::from(!set)),
    };
    Some(states)
}

/// The store id a placement predicts locally, when its placed state is certain.
pub fn predicted_placement(
    collisions: &PhysicsCollisionRegistries,
    stream: &impl crate::GameplayWorld,
    item_block: i32,
) -> Option<u32> {
    let resolved = held_block_store_id(stream, item_block)?;
    let mode = stream.network_id_mode();
    placement_state_is_certain(
        collisions.block_is_full_cube(mode, resolved),
        collisions.block_canonical_state(mode, resolved),
        collisions.block_identifier(mode, resolved),
    )
    .then_some(resolved)
}

pub fn held_block_store_id(stream: &impl crate::GameplayWorld, item_block: i32) -> Option<u32> {
    // Vanilla block-item descriptors preserve all runtime-id bits.
    // Our signed retained field is not a negative-id validity check: hashes may set bit 31.
    let block = u32::from_ne_bytes(item_block.to_ne_bytes());
    if block == 0 || block == u32::MAX {
        return None;
    }
    let resolved = stream.resolve_block_network_id(block);
    (resolved != stream.air_block_id()).then_some(resolved)
}

/// Only a stateless full cube places as the held state itself: oriented, sized
/// and merging blocks resolve their state from the click, which is not modelled.
/// Vanilla offsets a nonreplaceable clicked block even when it has the same
/// type as the held cube, then sets the block locally.
pub fn placement_state_is_certain(
    full_cube: bool,
    canonical_state: Option<&str>,
    placed_identifier: Option<&str>,
) -> bool {
    let stateless = canonical_state
        .and_then(|state| {
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(state).ok()
        })
        .is_some_and(|states| states.is_empty());
    full_cube && stateless && placed_identifier.is_some()
}

/// A successful hold starts once, then swings before its transaction and inventory delta.
#[allow(clippy::too_many_arguments)]
pub fn use_packets(
    (observed, block_network_id): (&FrozenBlockObservation, u32),
    player_position: [f32; 3],
    trigger: ItemUseTrigger,
    local_use: LocalUse,
    start_destination: Option<[i32; 3]>,
    change: Option<protocol::PredictedSlotChange>,
    local_runtime_id: u64,
    mut try_swing: impl FnMut(u64) -> bool,
    tick: u64,
) -> Vec<protocol::Packet> {
    let mut packets = Vec::with_capacity(3);
    if local_use != LocalUse::Nothing
        && let Some(destination) = start_destination
    {
        packets.push(protocol::start_item_use_on_packet(
            local_runtime_id,
            observed.target.position,
            destination,
            observed.target.face,
        ));
    }
    if let Some(source) = local_use.swing().filter(|_| try_swing(tick)) {
        packets.push(protocol::swing_arm_packet(local_runtime_id, source));
    }
    let request = BlockUseRequest {
        block_position: observed.target.position,
        face: observed.target.face,
        selected_slot: observed.selection.slot,
        selected_item: observed.selection.item.clone(),
        player_position,
        relative_hit: observed.target.relative_hit,
        block_runtime_id: u64::from(block_network_id),
    };
    let predicted = local_use != LocalUse::Nothing;
    if let Ok(packet) =
        protocol::click_block_transaction_packet(request, trigger, predicted, change)
    {
        packets.push(packet);
    }
    packets
}

#[cfg(test)]
mod tests;

mod respawn_anchor;

mod admission;
mod intention;
pub use intention::{BuildIntention, PlacementTarget, orientation_sensitive};

mod packets;
pub use packets::HeldPlacementInventory;

mod stopping;

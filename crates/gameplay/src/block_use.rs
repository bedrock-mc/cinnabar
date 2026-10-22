//! Block-use classification, held-repeat state and ordered packet production.
use crate::{interaction_authority::FrozenBlockObservation, movement::PhysicsCollisionRegistries};
use assets::{
    facts::{BlockFact, BlockProperty as P},
    registry::{BlockTraits as T, ModelFamily},
};
use client_world::game_mode_capabilities::GameModeCapabilities;
use protocol::{BlockUseRequest, ItemUseTrigger, SwingSource, VerifiedNetworkItemStack};
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};
use std::time::Duration;

// Held repeats are checked on simulation ticks against a monotonic deadline.
const SLOW_REPEAT_MILLIS: u64 = 300;
const STILL_REPEAT_MILLIS: u64 = 200;
const MOVING_REPEAT_MAX_MILLIS: u64 = 180;
/// Moving repeats wait this many milliseconds per block/second of speed.
const MOVING_REPEAT_BLOCK_MILLIS: f32 = 900.0;
const SURVIVAL_REPEAT_FLOOR_MILLIS: u64 = 100;
/// How far behind now a moving repeat's schedule may lag.
const REPEAT_MAX_LAG: Duration = Duration::from_millis(180);
/// Placement boxes shrink by this much per side before the actor test.
const PLACEMENT_ACTOR_EPSILON: f32 = 1.0e-5;

/// Which ability a block's own use needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Interaction {
    /// Doors, trapdoors, fence gates, buttons and levers.
    Switch,
    Container,
    /// Uses no ability gates.
    Other,
}

/// Selects the ability gate from the compiled interaction traits.
fn interaction(fact: BlockFact) -> Option<Interaction> {
    if fact.traits.contains(T::TOGGLES_BY_HAND) {
        Some(Interaction::Switch)
    } else if fact.traits.contains(T::OPENS_CONTAINER) {
        Some(Interaction::Container)
    } else if fact.traits.contains(T::USES_LOCALLY) {
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
pub(crate) fn overlaps(
    cell: [i32; 3],
    (local_min, local_max): BoxBounds,
    (min, max): BoxBounds,
) -> bool {
    (0..3).all(|axis| {
        let low = cell[axis] as f32 + local_min[axis] as f32 + PLACEMENT_ACTOR_EPSILON;
        let high = cell[axis] as f32 + local_max[axis] as f32 - PLACEMENT_ACTOR_EPSILON;
        low < max[axis] as f32 && (min[axis] as f32) < high
    })
}

const FULL_CELL: BoxBounds = ([0.0; 3], [1.0; 3]);

/// Empty vanilla buckets may pick liquid sources without making liquids selectable for mining.
pub fn uses_liquid_pick(item: &VerifiedNetworkItemStack, identifier: Option<&str>) -> bool {
    !item.is_empty()
        && item.metadata() == 0
        && identifier == Some(assets::facts::EMPTY_BUCKET_IDENTIFIER)
}

/// World facts one local use depends on.
#[derive(Debug, Clone, PartialEq)]
pub struct UseSurroundings {
    pub clicked: BlockFact,
    /// Canonical block state used to classify state-dependent block interactions.
    pub clicked_canonical_state: Option<String>,
    /// The selected block item's resolved block type, including server remaps.
    pub held: BlockFact,
    /// Facts for the cell across the clicked face; unresolved when unreadable.
    pub neighbor: BlockFact,
    pub player_box: BoxBounds,
    /// Boxes of actors that obstruct placement.
    pub actor_boxes: Vec<BoxBounds>,
    pub sneaking: bool,
    /// Block-local collision boxes of the held block; `None` when unknown, which
    /// tests the whole cell.
    pub placed_boxes: Option<Vec<BoxBounds>>,
}

impl UseSurroundings {
    /// Samples the held block's collision boxes in the resolved placement cell.
    pub fn set_placed_collision_shapes(
        &mut self,
        registry: &sim::CollisionRegistry,
        held: u32,
        clicked: [i32; 3],
        face: u8,
    ) {
        self.placed_boxes =
            placed_collision_boxes(registry, held, self.destination(clicked, face).0);
    }

    /// The cell a placement fills: the clicked block when it is replaceable,
    /// otherwise the neighbor across the clicked face.
    pub fn destination(&self, clicked: [i32; 3], face: u8) -> ([i32; 3], bool) {
        if self.clicked.traits.contains(T::REPLACEABLE) {
            (clicked, true)
        } else {
            let cell = placement_cell(clicked, face);
            (cell, self.neighbor.traits.contains(T::REPLACEABLE))
        }
    }
}

/// Block-local collision bounds sampled for a placement at `position`.
pub(crate) fn placed_collision_boxes(
    registry: &sim::CollisionRegistry,
    block: u32,
    position: [i32; 3],
) -> Option<Vec<BoxBounds>> {
    let world_offset = registry.block_shape_offset(block, position)?;
    let offset = world_offset
        - sim::Vec3::new(
            f64::from(position[0]),
            f64::from(position[1]),
            f64::from(position[2]),
        );
    Some(
        registry
            .collision_shapes(block)?
            .iter()
            .map(|shape| {
                let min = shape.min + offset;
                let max = shape.max + offset;
                ([min.x, min.y, min.z], [max.x, max.y, max.z])
            })
            .collect(),
    )
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
        let holding = item.network_id() != 0 && item.count() > 0;
        // Sneaking with an item uses the item instead of the block.
        let block_interaction = if surroundings.clicked.traits.contains(T::RESPAWN_ANCHOR) {
            respawn_anchor::uses_block(surroundings).then_some(Interaction::Other)
        } else {
            interaction(surroundings.clicked)
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

    /// An actor use consumes the press only when the target offers an interaction;
    /// otherwise the press falls through to item use after the interact transaction.
    /// Players have no client-side interaction, so the server's interact text alone
    /// decides; other actors' component interactions are not modelled and always interact.
    pub const fn for_actor(target: &protocol::ActorKind, interact_text: bool) -> Self {
        if interact_text || !matches!(target, protocol::ActorKind::Player { .. }) {
            Self::Interact
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
    last_use: Option<Duration>,
    /// The last success was a block interaction.
    slow_repeat: bool,
    last_attempt_tick: Option<u64>,
    /// Tick whose block interaction consumes the item-use press.
    interacted_tick: Option<u64>,
    /// The latest press was resolved as a block interaction.
    press_interacted: bool,
    position_authority: Option<(SessionGeneration, PositionAuthorityGeneration)>,
    pub intention: BuildIntention,
    selected_item: Option<(u8, i32, i32)>,
    rejected_tick: Option<u64>,
}

/// One held-use repeat's timing inputs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RepeatClock {
    /// Monotonic elapsed time, retaining the caller's sub-millisecond precision.
    pub now: Duration,
    pub sneaking: bool,
    /// Full 3D speed in blocks per second.
    pub speed: f32,
    pub survival: bool,
}

impl RepeatClock {
    /// Noncreative modes share the survival repeat floor and pick reach.
    pub fn for_game_mode(
        now: Duration,
        sneaking: bool,
        speed: f32,
        game_mode: Option<protocol::PlayerGameMode>,
    ) -> Self {
        Self {
            now,
            sneaking,
            speed,
            survival: game_mode != Some(protocol::PlayerGameMode::Creative),
        }
    }

    /// Times a build action from the tick-end state it observes.
    pub fn for_state(
        now: Duration,
        state: &crate::movement::UnsentSampleView,
        game_mode: Option<protocol::PlayerGameMode>,
    ) -> Self {
        let speed = state
            .displacement
            .map(|axis| axis * simulation_clock::TICKS_PER_SECOND as f32)
            .into_iter()
            .map(|axis| axis * axis)
            .sum::<f32>()
            .sqrt();
        Self::for_game_mode(now, state.sneaking, speed, game_mode)
    }
}

impl BlockUseRuntime {
    /// Returns the successful destination retained until the held action stops.
    pub fn last_success_destination(&self) -> Option<[i32; 3]> {
        self.intention.last_success_destination()
    }

    /// A slot or item-type change revokes a refused attempt while preserving the held action.
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
        due: Duration,
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
    pub fn synchronize(&mut self, authority: (SessionGeneration, PositionAuthorityGeneration)) {
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
    pub fn due(
        &self,
        held: bool,
        tick: u64,
        clock: RepeatClock,
    ) -> Option<(ItemUseTrigger, Duration)> {
        if self.waiting_for_authority {
            return None;
        }
        if self.latched_press {
            return Some((ItemUseTrigger::PlayerInput, clock.now));
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
        let due = self.last_use.map_or(Duration::ZERO, |last| {
            last.saturating_add(Duration::from_millis(interval))
        });
        (clock.now > due).then_some((ItemUseTrigger::SimulationTick, due))
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
        due: Duration,
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
        self.last_use = Some(match trigger {
            ItemUseTrigger::SimulationTick if moving => {
                due.max(clock.now.saturating_sub(REPEAT_MAX_LAG))
            }
            ItemUseTrigger::SimulationTick | ItemUseTrigger::PlayerInput => clock.now,
        });
        self.slow_repeat = local_use == LocalUse::Interact;
    }
}

/// The cell and state a switch use predicts within the observed world.
pub fn predicted_toggle(
    collisions: &PhysicsCollisionRegistries,
    mode: assets::NetworkIdMode,
    world: &sim::PaletteWorld<'_>,
    position: [i32; 3],
    clicked: u32,
) -> Option<([i32; 3], u32)> {
    if world.primary_runtime_id(position).ok()? != clicked {
        return None;
    }
    let fact = collisions.block_facts(mode, clicked);
    if fact.family == ModelFamily::Door {
        return predicted_door_toggle(collisions, mode, world, position, clicked);
    }
    let states = toggled_states(fact, collisions.block_canonical_state(mode, clicked)?)?;
    let block = collisions.block_state_runtime_id(
        mode,
        assets::facts::BlockStateIdentity::Compiled(
            collisions.block_facts(mode, clicked).identity?,
        ),
        &states,
    )?;
    Some((position, block))
}

/// Resolves both door cells and changes only the lower half's effective open state.
fn predicted_door_toggle(
    collisions: &PhysicsCollisionRegistries,
    mode: assets::NetworkIdMode,
    world: &sim::PaletteWorld<'_>,
    position: [i32; 3],
    clicked: u32,
) -> Option<([i32; 3], u32)> {
    let fact = collisions.block_facts(mode, clicked);
    if !fact.traits.contains(T::TOGGLES_BY_HAND) {
        return None;
    }
    let clicked_states =
        serde_json::from_str(collisions.block_canonical_state(mode, clicked)?).ok()?;
    let upper = toggle_bit(fact, &clicked_states, P::UpperBlockBit)?;
    let mut neighbor = position;
    neighbor[1] = neighbor[1].checked_add(if upper { -1 } else { 1 })?;
    let partner = world.primary_runtime_id(neighbor).ok()?;
    if collisions.block_facts(mode, partner).identity != fact.identity {
        return None;
    }
    let (lower_position, lower) = if upper {
        (neighbor, partner)
    } else {
        (position, clicked)
    };
    let mut states = serde_json::from_str(collisions.block_canonical_state(mode, lower)?).ok()?;
    let open = toggle_bit(fact, &states, P::OpenBit)?;
    crate::placement_state::set_bit(&mut states, P::OpenBit.name(), !open)?;
    let block = collisions.block_state_runtime_id(
        mode,
        assets::facts::BlockStateIdentity::Compiled(fact.identity?),
        &states,
    )?;
    Some((lower_position, block))
}

/// Reads a generated boolean domain without changing its canonical representation.
fn toggle_bit(
    fact: BlockFact,
    states: &serde_json::Map<String, serde_json::Value>,
    property: P,
) -> Option<bool> {
    crate::placement_state::valid_property(fact, states, property)?;
    let value = crate::placement_state::value(states, property.name())?;
    value
        .as_bool()
        .or_else(|| value.as_u64().map(|bit| bit != 0))
}

/// Trapdoors and levers flip `open_bit`; an unpressed button presses.
/// Door use needs the paired world cells and is resolved by `predicted_toggle`.
/// Fence gates still wait for the server's facing update.
pub fn toggled_states(
    fact: BlockFact,
    canonical_state: &str,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    if !fact.traits.contains(T::TOGGLES_BY_HAND) {
        return None;
    }
    let (property, press_only) = match fact.family {
        ModelFamily::Lever | ModelFamily::Trapdoor => (P::OpenBit.name(), false),
        ModelFamily::Button => (P::ButtonPressedBit.name(), true),
        _ => return None,
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

pub use crate::placement_prediction::{PlacementContext, PredictedPlacement, predicted_placement};

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
        hand: protocol::ItemUseHand::Mainhand,
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

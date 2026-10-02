//! Block use as standalone click-block transactions on the press and while held.
//!
//! The local use outcome (interaction, placement or nothing) decides the
//! transaction's prediction and swing. A placement or switch toggle whose state
//! is certain is also applied locally; the server's block updates stay authoritative. Air
//! use lives in `item_use`; item-use-on start/stop actions are not implemented.

use bevy::{
    ecs::system::SystemParam,
    prelude::{Query, Real, Res, ResMut, Resource, Time, Window, With},
    window::PrimaryWindow,
};
use protocol::{
    BlockUseRequest, ItemUseTrigger, PlayerGameMode, PlayerInputMode, SwingSource,
    VerifiedNetworkItemStack,
};
use semantic_input::Action;
use sim::PaletteWorld;

use crate::{
    game_mode_capabilities::GameModeCapabilities,
    interaction_authority::{FrozenBlockObservation, observe_block, within_pick_range},
    local_player::InteractionOriginSnapshot,
    melee::{MeleeRuntime, SwingTracker, obstructs_placement, swing_duration},
    menu::MenuRuntime,
    mining::{
        FrozenMiningSelection, creative_reach, protocol_input_mode, survival_reach,
        verified_selection,
    },
    movement::{LocalMovementEffectTimeline, MovementTicker, PhysicsCollisionRegistries},
    runtime::{network::NetworkHandle, world::ClientWorld},
    semantic_controls::SemanticInputSnapshot,
    ui_runtime::UiRuntime,
};

// Held-use repeat timing is wall-clock. All values need independent measurement.
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
pub(crate) fn repeat_interval_millis(
    sneaking: bool,
    slow: bool,
    speed: f32,
    survival: bool,
) -> u64 {
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
pub(crate) const fn placement_cell(clicked: [i32; 3], face: u8) -> [i32; 3] {
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
pub(crate) type BoxBounds = ([f64; 3], [f64; 3]);

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
pub(crate) struct UseSurroundings {
    pub(crate) clicked_identifier: Option<String>,
    /// Identifier of the cell across the clicked face; `None` when unreadable.
    pub(crate) neighbor_identifier: Option<String>,
    pub(crate) player_box: BoxBounds,
    /// Boxes of actors that obstruct placement.
    pub(crate) actor_boxes: Vec<BoxBounds>,
    pub(crate) sneaking: bool,
    /// Block-local collision boxes of the held block; `None` when unknown, which
    /// tests the whole cell.
    pub(crate) placed_boxes: Option<Vec<BoxBounds>>,
}

impl UseSurroundings {
    /// The cell a placement fills: the clicked block when it is replaceable,
    /// otherwise the neighbor across the clicked face.
    pub(crate) fn destination(&self, clicked: [i32; 3], face: u8) -> ([i32; 3], bool) {
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
pub(crate) enum LocalUse {
    Interact,
    Place,
    Nothing,
}

impl LocalUse {
    /// A block use the capabilities deny falls through to item use, as vanilla's does.
    pub(crate) fn resolve(
        item: &VerifiedNetworkItemStack,
        clicked: [i32; 3],
        face: u8,
        surroundings: &UseSurroundings,
        caps: &GameModeCapabilities,
    ) -> Self {
        let clicked_identifier = surroundings.clicked_identifier.as_deref();
        let holding = item.network_id() != 0 && item.count() > 0;
        // Sneaking with an item uses the item instead of the block.
        if clicked_identifier
            .and_then(interaction)
            .is_some_and(|interaction| interaction.permitted(caps))
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
#[derive(Resource, Debug, Default)]
pub(crate) struct BlockUseRuntime {
    latched_press: bool,
    last_use_millis: Option<u64>,
    /// The last success was an interaction or a not-yet-lined placement.
    slow_repeat: bool,
    last_attempt_tick: Option<u64>,
    /// Tick whose use press interacted with a block, which starts no item use.
    interacted_tick: Option<u64>,
    position_authority: Option<(u64, u64)>,
}

/// One held-use repeat's timing inputs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RepeatClock {
    pub(crate) now_millis: u64,
    pub(crate) sneaking: bool,
    /// Full 3D speed in blocks per second.
    pub(crate) speed: f32,
    pub(crate) survival: bool,
}

impl BlockUseRuntime {
    fn clear(&mut self) {
        self.latched_press = false;
        self.slow_repeat = false;
    }

    /// Latches an eligible use press until a physics tick can attempt it.
    fn observe_use(&mut self, held: bool, pressed: bool, attacking: bool) -> bool {
        if attacking || !(held || pressed || self.latched_press) {
            self.clear();
            return false;
        }
        self.latched_press |= pressed;
        true
    }

    /// Commits the use schedule after transport admission, retaining refused presses for retry.
    fn admit(
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

    /// A session or position-authority change drops the press and the schedule.
    pub(crate) fn synchronize(&mut self, authority: (u64, u64)) {
        if self
            .position_authority
            .is_some_and(|previous| previous != authority)
        {
            *self = Self::default();
        }
        self.position_authority = Some(authority);
    }

    /// The trigger due now, with the repeat's due time; at most one attempt per tick.
    pub(crate) fn due(
        &self,
        held: bool,
        tick: u64,
        clock: RepeatClock,
    ) -> Option<(ItemUseTrigger, u64)> {
        if self.latched_press {
            return Some((ItemUseTrigger::PlayerInput, clock.now_millis));
        }
        if !held || self.last_attempt_tick == Some(tick) {
            return None;
        }
        let interval = repeat_interval_millis(
            clock.sneaking,
            self.slow_repeat,
            clock.speed,
            clock.survival,
        );
        let due = self
            .last_use_millis
            .map_or(0, |last| last.saturating_add(interval));
        (clock.now_millis > due).then_some((ItemUseTrigger::SimulationTick, due))
    }

    /// Whether the use press resolved on `tick` interacted with a block.
    pub(crate) fn interacted_at(&self, tick: u64) -> bool {
        self.interacted_tick == Some(tick)
    }

    /// Records an attempt. As in vanilla, a failed repeat keeps its schedule, so it
    /// retries (and resends its transaction) on the next tick.
    pub(crate) fn record(
        &mut self,
        trigger: ItemUseTrigger,
        due: u64,
        tick: u64,
        local_use: LocalUse,
        clock: RepeatClock,
    ) {
        self.latched_press = false;
        self.last_attempt_tick = Some(tick);
        if trigger == ItemUseTrigger::PlayerInput && local_use == LocalUse::Interact {
            self.interacted_tick = Some(tick);
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
        // A placement line is treated as established after its first repeat.
        self.slow_repeat = local_use == LocalUse::Interact
            || (local_use == LocalUse::Place && trigger == ItemUseTrigger::PlayerInput);
    }
}

#[derive(SystemParam)]
pub(crate) struct BlockUseContext<'w, 's> {
    input: Res<'w, SemanticInputSnapshot>,
    origin: Res<'w, InteractionOriginSnapshot>,
    ui: Res<'w, UiRuntime>,
    menu: Res<'w, MenuRuntime>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    client_world: ResMut<'w, ClientWorld>,
    collisions: Res<'w, PhysicsCollisionRegistries>,
    effects: Res<'w, LocalMovementEffectTimeline>,
    melee: Res<'w, MeleeRuntime>,
    network: Res<'w, NetworkHandle>,
    time: Res<'w, Time<Real>>,
    audio_cues: bevy::prelude::MessageWriter<'w, crate::audio::LocalBlockCue>,
}

pub(crate) fn produce_block_use(
    mut context: BlockUseContext,
    mut runtime: ResMut<BlockUseRuntime>,
    mut swings: ResMut<SwingTracker>,
    movement: Res<MovementTicker>,
) {
    runtime.synchronize(movement.interaction_authority_identity());
    let focused =
        !context.menu.is_visible() && context.windows.single().is_ok_and(|window| window.focused);
    let game_mode = context.ui.player_game_mode();
    let caps = context.ui.game_mode_capabilities();
    let Some((input, caps)) = context.input.snapshot().zip(caps).filter(|(input, caps)| {
        focused
            && !context.ui.ui_focused()
            && caps.can_use_blocks()
            && input.input_mode != semantic_input::InputMode::Touch
            && movement.accepts_block_interactions()
    }) else {
        runtime.clear();
        return;
    };
    let use_phase = context.input.phase(Action::Use);
    if !runtime.observe_use(
        use_phase.held,
        use_phase.pressed,
        context.input.phase(Action::Attack).held,
    ) {
        return;
    }
    // Frames between physics ticks have no unsent tick; a press waits for one.
    let Some(sample) = movement.newest_unsent_sample() else {
        return;
    };
    let clock = RepeatClock {
        now_millis: u64::try_from(context.time.elapsed().as_millis()).unwrap_or(u64::MAX),
        sneaking: sample.sneaking,
        speed: sample
            .delta
            .map(|axis| axis * sim::TICKS_PER_SECOND as f32)
            .into_iter()
            .map(|axis| axis * axis)
            .sum::<f32>()
            .sqrt(),
        survival: game_mode == Some(PlayerGameMode::Survival),
    };
    let Some((trigger, due)) = runtime.due(use_phase.held, sample.tick, clock) else {
        return;
    };
    if context.melee.blocks_use_at(clock.now_millis) {
        runtime.latched_press = false;
        return;
    }
    let input_mode = protocol_input_mode(input.input_mode);
    let (Some(observed), Some(stream)) = (
        observe_use_target(
            &context,
            input_mode,
            clock.survival,
            (input.authority_generation, input.frame_sequence),
            movement.interaction_authority_identity().1,
        ),
        context.client_world.stream.as_ref(),
    ) else {
        runtime.record(trigger, due, sample.tick, LocalUse::Nothing, clock);
        return;
    };
    let surroundings = use_surroundings(&context, &observed, sample.position, sample.sneaking);
    let local_use = LocalUse::resolve(
        &observed.selection.item,
        observed.target.position,
        observed.target.face,
        &surroundings,
        &caps,
    );
    let (destination, _) = surroundings.destination(observed.target.position, observed.target.face);
    let predicted = (local_use == LocalUse::Place)
        .then(|| {
            predicted_placement(
                &context.collisions,
                stream,
                observed.selection.item.block_runtime_id(),
            )
        })
        .flatten()
        .map(|block| (destination, block));
    let predicted = predicted.or_else(|| {
        (local_use == LocalUse::Interact)
            .then(|| predicted_toggle(&context.collisions, stream, observed.target.runtime_id))
            .flatten()
            .map(|block| (observed.target.position, block))
    });
    let local_runtime_id = stream.local_player_runtime_id();
    // Only block items keep using while held.
    if trigger == ItemUseTrigger::SimulationTick && observed.selection.item.block_runtime_id() == 0
    {
        runtime.record(trigger, due, sample.tick, local_use, clock);
        return;
    }
    let duration = swing_duration(context.effects.mining_effects());
    let before_swing = swings.clone();
    let packets = use_packets(
        &observed,
        sample.position,
        trigger,
        local_use,
        local_runtime_id,
        |tick| swings.try_swing(tick, duration),
        sample.tick,
    );
    let sent = !packets.is_empty() && context.network.send_inventory_packets(packets).is_ok();
    if !runtime.admit(trigger, due, sample.tick, local_use, clock, sent) {
        *swings = before_swing;
        return;
    }
    if local_use == LocalUse::Place {
        let position = destination;
        context
            .audio_cues
            .write(crate::audio::LocalBlockCue::Place {
                position,
                block_runtime_id: observed.selection.item.block_runtime_id(),
            });
    }
    // Vanilla places locally as it sends; a correction replaces the prediction.
    if let (true, Some((position, block)), Some(stream)) =
        (sent, predicted, context.client_world.stream.as_mut())
    {
        let applied = stream.predict_block(position, 0, block);
        bevy::log::debug!(
            ?position,
            block,
            applied,
            "local block prediction committed"
        );
    }
}

/// The state a switch use predicts for the clicked block.
fn predicted_toggle(
    collisions: &PhysicsCollisionRegistries,
    stream: &client_world::WorldStream,
    clicked: u32,
) -> Option<u32> {
    let mode = stream.network_id_mode();
    let identifier = collisions.block_identifier(mode, clicked)?;
    let states = toggled_states(identifier, collisions.block_canonical_state(mode, clicked)?)?;
    collisions.block_state_runtime_id(mode, identifier, &states)
}

/// Trapdoors and levers flip `open_bit` (`TrapDoorBlock::_useTrapDoor`); an
/// unpressed button presses. Doors and fence gates also change their other
/// half or facing, which is not modelled, so they wait for the server.
fn toggled_states(
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
fn predicted_placement(
    collisions: &PhysicsCollisionRegistries,
    stream: &client_world::WorldStream,
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

fn held_block_store_id(stream: &client_world::WorldStream, item_block: i32) -> Option<u32> {
    // BlockItem's descriptor (native RVA 0x09cc4df0) preserves all runtime-id bits.
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
/// Native RVA 0x09cc4aa0 offsets a nonreplaceable clicked block even when it has
/// the same type as the held cube; RVA 0x09cc3660 then sets the block locally.
fn placement_state_is_certain(
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

/// A successful local use swings before its transaction, which is always sent.
pub(crate) fn use_packets(
    observed: &FrozenBlockObservation,
    player_position: [f32; 3],
    trigger: ItemUseTrigger,
    local_use: LocalUse,
    local_runtime_id: u64,
    mut try_swing: impl FnMut(u64) -> bool,
    tick: u64,
) -> Vec<protocol::Packet> {
    let mut packets = Vec::with_capacity(2);
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
        block_runtime_id: u64::from(observed.target.runtime_id),
    };
    let predicted = local_use != LocalUse::Nothing;
    if let Ok(packet) = protocol::click_block_transaction_packet(request, trigger, predicted) {
        packets.push(packet);
    }
    packets
}

fn use_surroundings(
    context: &BlockUseContext,
    observed: &FrozenBlockObservation,
    network_position: [f32; 3],
    sneaking: bool,
) -> UseSurroundings {
    let stream = context.client_world.stream.as_ref();
    let identifier = |position: [i32; 3]| -> Option<String> {
        let stream = stream?;
        let mode = stream.network_id_mode();
        let world = PaletteWorld::new(
            stream.collision_store(),
            context.collisions.registry(mode),
            stream.current_dimension(),
        );
        if world.is_air(position).ok()? {
            return Some("minecraft:air".to_owned());
        }
        let runtime_id = world.primary_runtime_id(position).ok()?;
        context
            .collisions
            .block_identifier(mode, runtime_id)
            .map(str::to_owned)
    };
    let feet = [
        f64::from(network_position[0]),
        f64::from(network_position[1] - protocol::PLAYER_NETWORK_OFFSET),
        f64::from(network_position[2]),
    ];
    let half_width = sim::PLAYER_WIDTH * 0.5;
    let height = sim::MovementMode::Walking.hitbox_height(sneaking);
    let placed_boxes = stream.and_then(|stream| {
        let shapes = context
            .collisions
            .registry(stream.network_id_mode())
            .collision_shapes(held_block_store_id(
                stream,
                observed.selection.item.block_runtime_id(),
            )?)?;
        Some(
            shapes
                .iter()
                .map(|shape| {
                    (
                        [shape.min.x, shape.min.y, shape.min.z],
                        [shape.max.x, shape.max.y, shape.max.z],
                    )
                })
                .collect(),
        )
    });
    UseSurroundings {
        clicked_identifier: context
            .collisions
            .block_identifier(
                stream.map_or(assets::NetworkIdMode::Sequential, |stream| {
                    stream.network_id_mode()
                }),
                observed.target.runtime_id,
            )
            .map(str::to_owned),
        neighbor_identifier: identifier(placement_cell(
            observed.target.position,
            observed.target.face,
        )),
        player_box: (
            [feet[0] - half_width, feet[1], feet[2] - half_width],
            [feet[0] + half_width, feet[1] + height, feet[2] + half_width],
        ),
        actor_boxes: stream
            .into_iter()
            .flat_map(|stream| stream.remote_actors())
            .filter(|actor| obstructs_placement(actor))
            .filter_map(|actor| actor.bounding_box())
            .map(|(min, max)| (min.map(f64::from), max.map(f64::from)))
            .collect(),
        sneaking,
        placed_boxes,
    }
}

fn observe_use_target(
    context: &BlockUseContext,
    input_mode: PlayerInputMode,
    survival: bool,
    input_authority: (std::num::NonZeroU64, u64),
    position_authority_generation: u64,
) -> Option<FrozenBlockObservation> {
    let reach = if survival {
        survival_reach(input_mode)
    } else {
        creative_reach(input_mode)
    };
    let observed = observe_block(
        &context.origin,
        &context.ui,
        &context.client_world,
        &context.collisions,
        verified_use_selection(&context.ui)?,
        (
            input_mode,
            reach,
            input_authority,
            position_authority_generation,
        ),
    )?;
    within_pick_range(&observed).then_some(observed)
}

/// The selected stack, only while no inventory request or hotbar change is in flight.
pub(crate) fn verified_use_selection(ui: &UiRuntime) -> Option<FrozenMiningSelection> {
    let ledger = ui.inventory_ledger();
    if ledger.pending_request_id().is_some()
        || ledger.resync_required()
        || ui.pending_hotbar_selection().is_some()
    {
        return None;
    }
    verified_selection(ui)
}

#[cfg(test)]
mod tests;

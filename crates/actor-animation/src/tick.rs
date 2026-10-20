use super::*;
use assets::EntityControllerAnimationTarget;
use protocol_ids::ActorFlag;

pub(super) mod controller;
pub(super) mod scale;
pub(super) mod selection;
use controller::ControllerWalk;

#[cfg(test)]
#[path = "tick_cape_tests.rs"]
mod cape_tests;

/// A held stack's data value, block and tags: a change re-equips the item, while its count and
/// network instance do not.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StackVariant {
    pub metadata: u32,
    pub block_runtime_id: i32,
    pub nbt_digest: [u8; 32],
}

impl From<&assets::ItemStackIdentity> for StackVariant {
    fn from(identity: &assets::ItemStackIdentity) -> Self {
        Self {
            metadata: identity.metadata,
            block_runtime_id: identity.block_runtime_id,
            nbt_digest: identity.nbt_digest,
        }
    }
}

/// Actor state beyond the snapshot that one tick's evaluation reads.
#[derive(Clone, Debug, Default)]
pub struct ActorTickContext {
    /// Visual attack rules; gameplay clocks remain independent of this profile.
    pub combat_visuals: render_api::CombatVisuals,
    /// Frame fraction supplied only to scratch render-layer evaluation.
    pub frame_alpha: f32,
    /// Full elapsed visual interval; absent for an explicit single-tick evaluation.
    pub animation_elapsed_ticks: Option<u32>,
    /// A known first appearance is still awaiting its immutable model preparation.
    pub appearance_pending: bool,
    pub is_riding: bool,
    /// Namespaced identifiers of the equipped main-hand and off-hand items.
    pub main_hand: Option<Arc<str>>,
    pub off_hand: Option<Arc<str>>,
    /// Negotiated facts for main hand and off hand, copied at context admission.
    pub held_item_facts: [assets::facts::ItemFact; 2],
    /// Stack facts beside each hand's identifier whose change re-equips it.
    pub held_variants: [StackVariant; 2],
    /// The main-hand stack's data value.
    pub main_hand_metadata: u32,
    /// A positive network identity distinguishes mutation from replacement of the held stack.
    pub main_hand_stack_id: Option<i32>,
    /// The selected hotbar slot, retained independently from the stack identity.
    pub main_hand_slot: u8,
    /// Current local Bedrock duration; zero retains the duration of a remote swing.
    pub bedrock_swing_ticks: i32,
    /// Current Java swing length after haste and fatigue.
    pub java_swing_ticks: i32,
    /// A held crossbow is loaded.
    pub hand_charged: bool,
    /// Ticks the main-hand item can be used for, or 0 when unknown.
    pub main_hand_max_use_ticks: u32,
    /// Main-hand consumption after server components and pack overrides are admitted.
    pub main_hand_consumption: assets::item_use::ItemConsumption,
    /// Selected-stack gameplay clock for the local rig; remote actors retain their own ticks.
    pub local_item_use_ticks: Option<u32>,
    /// Maximum duration of the owner's offhand stack for explicitly selected-slot queries.
    pub off_hand_max_use_ticks: u32,
    /// Selected kinetic weapon component timings used by its authored pose.
    pub main_hand_kinetic: Option<protocol::KineticWeaponTiming>,
    /// The selected item declares the native spear animation tag.
    pub main_hand_is_spear: bool,
    /// Selected melee component swing duration, in seconds.
    pub main_hand_swing_seconds: Option<f32>,
    /// Namespaced identifier of the actor being ridden.
    pub ridden: Option<Arc<str>>,
    pub has_rider: bool,
    pub has_player_rider: bool,
    /// The local player rendered from its own camera; selects the first-person render controller.
    pub is_local_first_person: bool,
    /// The actor belongs to this game window, including third-person and HUD views.
    pub is_local_player: bool,
    /// Local view-bobbing preference; other actor contexts keep the native default.
    pub view_bobbing: Option<bool>,
    /// The client's own player.
    pub is_local: bool,
    /// The client's own player is flying.
    pub is_flying: bool,
    /// The committed level default resolves players that use the default mode.
    pub world_game_mode: Option<protocol::PlayerGameMode>,
    /// Native HUD rendering uses a UI actor context without a first-person hand camera.
    pub is_in_ui: bool,
    /// `[pitch, yaw]` of the view in degrees, for camera-facing billboards.
    pub camera_rotation: [f32; 2],
    /// World position of the view, for camera-relative queries.
    pub camera_position: [f32; 3],
    /// Worn stacks in helmet, chestplate, leggings, boots, body order.
    pub armor: [Option<WornArmor>; 5],
    /// The player's skin carries a cape image.
    pub has_cape: bool,
    /// The player's skin model inputs, when it may name its own geometry.
    pub skin_geometry: Option<Arc<render_api::SkinGeometrySource>>,
    /// The actor type's synced property definitions, in wire index order.
    pub properties: Option<Arc<[PropertyDefinition]>>,
    /// Item-render query units and contexts differ from ordinary actor queries.
    pub attachable: Option<super::attachable::AttachableQueryContext>,
}

/// One worn armor stack as the armor queries read it.
#[derive(Clone, Debug)]
pub struct WornArmor {
    pub item: Arc<str>,
    pub fact: assets::facts::ItemFact,
    pub dye_rgb: Option<u32>,
}

// Fraction of full swim posture gained or lost per tick; needs independent measurement.
const SWIM_AMOUNT_STEP: f32 = 0.2;

// Vanilla held-item tick: ±0.4 clamp and cached
// stack replacement at height <= 0.1.
const ARM_HEIGHT_STEP: f32 = 0.4;
const ARM_SWAP_HEIGHT: f32 = 0.1;

// Vanilla applies this modified-speed query multiplier to babies.
const BABY_MOVE_SPEED_SCALE: f32 = 1.5;

// Gliding divides limb swing by the cubed squared speed over this; needs independent
// measurement.
const GLIDING_SPEED_SQUARED_UNIT: f32 = 0.2;

/// Advances the walk cycle, swing, and body yaw every tick, whether or not the rig's Molang
/// runs, so static and failing rigs still turn and move.
pub(super) fn advance_motion(
    state: &mut ActorRigState,
    actor: &dyn AnimatedActor,
    context: &ActorTickContext,
    reset_history: bool,
) {
    state.presentation_seeded = false;
    if reset_history && state.reset_pending {
        state.history.clear();
    }
    let previous_position = state
        .history
        .back()
        .map_or(actor.position(), |input| input.position);
    let position_delta =
        std::array::from_fn(|axis| actor.position()[axis] - previous_position[axis]);
    let motion = &mut state.motion;
    if context.bedrock_swing_ticks > 0 {
        motion.set_swing_duration(context.bedrock_swing_ticks);
    }
    motion.advance(&MotionInput {
        delta: position_delta,
        riding: context.is_riding,
        player: matches!(actor.kind(), ActorKind::Player { .. }),
        yaw: actor.yaw(),
        head_yaw: actor.head_yaw(),
        on_fire: actor.is_on_fire(),
    });
    if is_native_fish(actor) {
        motion.advance_fish(actor.native_velocity());
    }
    if actor.is_horse() {
        motion
            .horse
            .advance(query::actor_flag(actor, ActorFlag::Standing.wire_value()));
    }
    // Arrow orientation is entirely in animation.arrow.move's body bone. It is not
    // a mob: its interpolated body yaw is 0, while
    // query.target_y_rotation reads the actor's absolute rotation.
    if query::is_arrow(actor) {
        motion.body_yaw = 0.0;
        motion.previous_body_yaw = 0.0;
    }
    let baby_scale = if query::actor_flag(actor, ActorFlag::Baby.wire_value()) {
        BABY_MOVE_SPEED_SCALE
    } else {
        1.0
    };
    let item_use_ticks = if let Some(ticks) = local_use_counter(actor, context) {
        ticks
    } else if query::actor_flag(actor, ActorFlag::UsingItem.wire_value()) {
        state
            .history
            .back()
            .map_or(0, |input| input.item_use_ticks)
            .saturating_add(1)
    } else {
        0
    };
    let swim_target = if query::actor_flag(actor, ActorFlag::Swimming.wire_value()) {
        1.0
    } else {
        0.0
    };
    let swim_amount = state.history.back().map_or(swim_target, |input| {
        let previous = input.swim_amount;
        previous + (swim_target - previous).clamp(-SWIM_AMOUNT_STEP, SWIM_AMOUNT_STEP)
    });
    // The arm lowers while the held item differs from the equipped one, swaps it low, then rises.
    let [main_variant, off_variant] = context.held_variants;
    let arm_height = match state.history.back() {
        Some(previous) => advance_equip_height(
            &mut state.equipped_main,
            (&context.main_hand, main_variant),
            previous.arm_height,
        ),
        None => {
            state.equipped_main = (context.main_hand.clone(), main_variant);
            1.0
        }
    };
    state.off_hand_animation[0] = state.off_hand_animation[1];
    let off_hand_arm_height = advance_equip_height(
        &mut state.equipped_off,
        (&context.off_hand, off_variant),
        state.off_hand_animation[0].arm_height,
    );
    state.off_hand_animation[1].arm_height = off_hand_arm_height;
    let java_held = context
        .main_hand
        .as_ref()
        .map(|identifier| super::java::JavaHeldItem {
            identifier: Arc::clone(identifier),
            metadata: context.main_hand_metadata,
            stack_id: context.main_hand_stack_id.filter(|id| *id > 0),
        });
    state.java.advance(&super::java::JavaTick {
        delta: position_delta,
        yaw: actor.yaw(),
        local_swing: state.local_swing.map(|progress| progress.java),
        swing_ticks: if context.java_swing_ticks > 0 {
            context.java_swing_ticks
        } else {
            ACTOR_SWING_TICKS
        },
        held: &java_held,
        held_slot: context.main_hand_slot,
        riding: context.is_riding,
        vanilla_posture: java_retains_native_posture(actor, context, swim_amount),
        position: actor.position(),
        velocity: actor.native_velocity(),
        on_ground: actor.on_ground(),
        alive: !actor.is_dead() && actor.health().is_none_or(|health| health.current > 0.0),
        sneaking: query::actor_flag(actor, ActorFlag::Sneaking.wire_value()),
        flying: context.is_flying,
        local: context.is_local,
    });
    if state.history.len() == MAX_ACTOR_ACTION_HISTORY {
        state.history.pop_front();
    }
    let input = ActorTickInput {
        placement_frame: state.placement_frame,
        position: actor.position(),
        position_delta,
        velocity: actor.velocity(),
        on_ground: actor.on_ground(),
        body_yaw: motion.body_yaw,
        yaw: actor.yaw(),
        head_yaw: actor.head_yaw(),
        pitch: actor.pitch(),
        is_riding: context.is_riding,
        distance_moved: motion.distance,
        move_speed: motion.speed.min(1.0) * baby_scale,
        walk_distance: motion.walk_distance(),
        item_use_ticks,
        swim_amount,
        arm_height,
        off_hand_arm_height,
        attack_time: motion.attack_time(),
    };
    state.history.push_back(input);
}

/// Seeds current snapshot inputs for a first draw without advancing any motion clock.
pub(super) fn seed_presentation_input(
    state: &mut ActorRigState,
    actor: &dyn AnimatedActor,
    context: &ActorTickContext,
) {
    let retained = state
        .history
        .back()
        .copied()
        .filter(|_| !state.presentation_seeded);
    if retained.is_none() {
        state.equipped_main = (context.main_hand.clone(), context.held_variants[0]);
    }
    let counters = retained.unwrap_or_else(|| ActorTickInput {
        swim_amount: f32::from(query::actor_flag(actor, ActorFlag::Swimming.wire_value())),
        arm_height: 1.0,
        off_hand_arm_height: state.off_hand_animation[1].arm_height,
        attack_time: state.motion.attack_time(),
        ..Default::default()
    });
    let input = ActorTickInput {
        placement_frame: state.placement_frame,
        position: actor.position(),
        velocity: actor.velocity(),
        on_ground: actor.on_ground(),
        body_yaw: if query::is_arrow(actor) {
            0.0
        } else {
            state.motion.body_yaw
        },
        yaw: actor.yaw(),
        head_yaw: actor.head_yaw(),
        pitch: actor.pitch(),
        is_riding: context.is_riding,
        item_use_ticks: local_use_counter(actor, context).unwrap_or(counters.item_use_ticks),
        ..counters
    };
    state.java.motion.riding = context.is_riding;
    state.java.motion.vanilla_posture =
        java_retains_native_posture(actor, context, input.swim_amount);
    if let Some(previous) = state.history.back_mut() {
        *previous = input;
    } else {
        state.history.push_back(input);
    }
    state.presentation_seeded = retained.is_none();
}

/// Keeps authored postures that the Java option does not replace.
fn java_retains_native_posture(
    actor: &dyn AnimatedActor,
    context: &ActorTickContext,
    swim_amount: f32,
) -> bool {
    swim_amount > 0.0
        || query::actor_flag(actor, ActorFlag::Swimming.wire_value())
        || context.main_hand_is_spear
        || query::actor_flag(actor, ActorFlag::Gliding.wire_value())
        || query::actor_flag(actor, ActorFlag::DamageNearbyMobs.wire_value())
        || query::actor_flag(actor, ActorFlag::Crawling.wire_value())
        || query::actor_flag(actor, ActorFlag::Emoting.wire_value())
        || actor.is_sleeping()
}

/// Converts the local elapsed clock to the existing one-based hand and spear counter.
pub(super) fn local_use_counter(
    actor: &dyn AnimatedActor,
    context: &ActorTickContext,
) -> Option<u32> {
    context.local_item_use_ticks.map(|elapsed| {
        if actor.is_using_item() {
            elapsed.saturating_add(1)
        } else {
            0
        }
    })
}

/// Identifier changes request the native lowering transition. Item-specific stack
/// equivalence/instant-update predicates need stack data beyond the current actor feed.
fn advance_equip_height(
    equipped: &mut (Option<Arc<str>>, StackVariant),
    (requested, variant): (&Option<Arc<str>>, StackVariant),
    previous_height: f32,
) -> f32 {
    let swapping = equipped.0 != *requested || (requested.is_some() && equipped.1 != variant);
    let target = if swapping { 0.0 } else { 1.0 };
    let height =
        previous_height + (target - previous_height).clamp(-ARM_HEIGHT_STEP, ARM_HEIGHT_STEP);
    if swapping && height <= ARM_SWAP_HEIGHT {
        *equipped = (requested.clone(), variant);
    }
    height
}

impl ActorRigState {
    /// The previous and current tick's swing and equip progress; rest before any tick.
    pub(super) fn hand_phases(&self) -> [HandPhase; 2] {
        let phase = |input: &ActorTickInput| HandPhase {
            attack_time: input.attack_time,
            arm_height: input.arm_height,
            use_ticks: input.item_use_ticks,
        };
        let mut recent = self.history.iter().rev().map(phase);
        let current = recent.next().unwrap_or_default();
        let mut phases = [recent.next().unwrap_or(current), current];
        if let Some(progress) = self.local_swing {
            for (phase, attack_time) in phases.iter_mut().zip(progress.bedrock) {
                phase.attack_time = attack_time;
            }
        }
        phases
    }

    /// A remote rig's previous and current attack progress while it swings.
    pub(super) fn remote_swing(&self) -> Option<[f32; 2]> {
        let pair = self.hand_phases().map(|phase| phase.attack_time);
        (self.local_swing.is_none() && pair != [0.0; 2]).then_some(pair)
    }
}

pub(super) struct EvaluationInheritance<'a> {
    pub variables: ActorAnimationVariables<'a>,
    pub overrides: &'a [(&'a str, f32)],
}

#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate_state(
    assets: &RuntimeEntityAssets,
    layout: &VariableLayout,
    state: &ActorRigState,
    actor: &dyn AnimatedActor,
    context: &ActorTickContext,
    tick: u64,
    budget: &mut EvalBudget<'_>,
    advance_clocks: bool,
    inheritance: Option<EvaluationInheritance<'_>>,
) -> Result<EvaluatedState, EvalError> {
    let replay = (!advance_clocks).then(|| state.replay_at(tick)).flatten();
    let topology = replay.filter(|replay| replay.geometry == state.geometry_binding);
    let reset = topology.map_or(state.reset_pending, |replay| replay.reset);
    let replay_context = replay.map(|replay| ActorTickContext {
        animation_elapsed_ticks: replay.elapsed,
        ..context.clone()
    });
    let context = replay_context.as_ref().unwrap_or(context);
    let anim_tick = if reset {
        0
    } else {
        tick.saturating_sub(topology.map_or(state.animation_epoch, |replay| replay.epoch))
    };
    let life_tick = tick.saturating_sub(state.lifetime_epoch);
    let observed = state.history.back().copied().ok_or(EvalError::Invalid)?;
    // The first-person draw zeroes the actor's rotations, so the view-following target, body
    // and head rotation queries read 0 there; the camera placement carries the view instead.
    let input = evaluation_input(observed, context);
    let motion = state.motion;
    let evaluator = Evaluator {
        assets,
        pack: state.pack,
        layout,
        program: None,
        actor,
        input: &input,
        context,
        anim_tick,
        anim_time: None,
        swell_amount: None,
        presentation_alpha: None,
        query_history: None,
        life_tick,
        finished: (false, false),
        state_time: 0.0,
        bones: state.posed_bones(),
        bone_names: state.posed_bone_names(),
    };
    let rig = assets
        .rig_bindings()
        .get(state.rig_binding)
        .ok_or(EvalError::Invalid)?;
    let mut variables = replay
        .map_or(&state.variables, |replay| &replay.variables)
        .clone();
    let engine = &layout.engine;
    if !replay.map_or(state.initialized, |replay| replay.initialized) {
        for &(slot, value) in &engine.seeded {
            variables.set(Some(slot), value);
        }
        if let Some(script) = rig.initialize {
            evaluator.run(script as usize, &mut variables, 0.0, budget)?;
        }
    }
    apply_engine_variables(engine, &mut variables, actor, context, &observed, &motion);
    super::skin_layers::seed(state, &evaluator, &mut variables);
    if let Some(inheritance) = inheritance {
        inheritance
            .variables
            .copy_to(assets, layout, &mut variables);
        inheritance.variables.sample_spear_to(
            &engine.spear,
            &mut variables,
            actor,
            &input,
            context
                .attachable
                .map_or(context.frame_alpha, |item| item.frame_alpha),
        );
        for &(name, value) in inheritance.overrides {
            variables.set(layout.named_slot(assets, name), value);
        }
    }
    attachable::apply_context_variables(engine, &mut variables, actor, context, &observed);
    variables.clear_temporaries();
    variables.clear(engine.first_person_item_rotation_factor);
    let samples_motion = state.motion_sampling.as_ref().is_some_and(|sampling| {
        actor.is_creeper()
            || sampling.uses_rotation()
            || sampling.uses_walk()
            || state.samples_camera_poses
            || state.samples_swing_poses
    });
    let mut render_frame = (state.samples_render_frames
        || samples_motion
        || (state.samples_swing_poses
            && (state.local_swing.is_some() || state.remote_swing().is_some())))
    .then(|| super::render_frame::FrameState {
        previous_rotation: if context.is_local_first_person {
            input
        } else {
            state.history.iter().rev().nth(1).copied().unwrap_or(input)
        },
        previous_walk: state.history.iter().rev().nth(1).copied().unwrap_or(input),
        samples_camera_poses: false,
        samples_swing_poses: false,
        selection_effects: evaluation::MolangEffects::default(),
        retained_pose_effects: evaluation::MolangEffects::default(),
        motion: super::render_frame::motion_endpoint::MotionEndpointState {
            variables: variables.clone(),
            sampling: state.motion_sampling.clone(),
            queries: if samples_motion {
                state
                    .motion_sampling
                    .as_ref()
                    .unwrap()
                    .freeze_queries(&evaluator)
            } else {
                Vec::new()
            },
            actor: (samples_motion && state.motion_sampling.as_ref().unwrap().samples_properties())
                .then(|| actor.to_shared()),
            context: context.clone(),
            input,
            anim_tick,
            life_tick,
            clips: Vec::new(),
            clocks: BTreeMap::new(),
            controllers: Vec::new(),
            journal: controller::ControllerJournal::default(),
            server_effects: evaluation::MolangEffects::default(),
        },
        previous_motion: None,
        motion_poses: None,
        motion_layers: BTreeMap::new(),
        swelling: [actor.creeper_swell_amount(context.frame_alpha); 2],
    });
    let held_item_scale = scale::evaluate_held_item(&evaluator, rig, &mut variables, budget)?;
    if let Some(script) = rig.pre_animation {
        let static_draw = budget.static_draw.take().map(|cacheable| {
            cacheable
                && super::attachable::static_draw::expression_is_static(
                    assets,
                    script as usize,
                    true,
                )
        });
        evaluator.run(script as usize, &mut variables, 0.0, budget)?;
        budget.static_draw = static_draw;
    }
    if state.complete_spear_variables {
        engine
            .spear
            .apply(&mut variables, actor, context, &input, input.attack_time);
    }
    let scale = scale::evaluate_model(&evaluator, rig, &mut variables, budget)?;
    set_item_rotation_factor(engine, &mut variables);
    let mut controllers = topology
        .map_or(&state.controllers, |replay| &replay.controllers)
        .clone();
    for controller in &mut controllers {
        controller.active = false;
    }
    let blink_controller = super::skin_layers::blink_controller(assets, state);
    if let Some(controller) = blink_controller
        && !controllers
            .iter()
            .any(|runtime| runtime.controller == controller)
    {
        collect_controllers(assets, controller, 0, &mut controllers).ok_or(EvalError::Invalid)?;
    }
    if reset {
        for runtime in &mut controllers {
            runtime.state = assets
                .controllers()
                .get(runtime.controller)
                .ok_or(EvalError::Invalid)?
                .initial_state;
            runtime.entered_tick = 0;
            runtime.blend_from = None;
        }
    }
    // Lifetime clocks survive a reset; only a resetting rig copies them out.
    let reset_clocks: super::clock::ClipClocks;
    let previous_clocks = if reset {
        reset_clocks = state
            .clip_clocks
            .iter()
            .filter(|((_, _, basis), _)| *basis == super::clock::Basis::Lifetime)
            .map(|(key, value)| (*key, *value))
            .collect();
        &reset_clocks
    } else {
        replay.map_or(&state.clip_clocks, |replay| &replay.clocks)
    };
    let retain_selection = render_frame.is_some();
    let mut journal = controller::ControllerJournal::new(retain_selection);
    let selection_capture = render_frame.as_ref().map(|_| variables.begin_effects());
    let mut weighted_clips = selection::select(
        &evaluator,
        &mut variables,
        &mut controllers,
        selection::Input {
            clocks: previous_clocks,
            geometry: state.geometry_binding,
            blink: blink_controller,
            journal: &mut journal,
            replay: false,
            record: retain_selection,
        },
        budget,
    )?;
    let mut server_animations = replay
        .map_or(&state.server_animations, |replay| &replay.server_animations)
        .clone();
    let server_capture = retain_selection.then(|| variables.begin_effects());
    super::server_animation::select(
        &evaluator,
        &mut variables,
        &mut server_animations,
        super::server_animation::SelectionInput {
            geometry: state.geometry_binding,
            clocks: previous_clocks,
            advance: advance_clocks || replay.is_some(),
        },
        &mut weighted_clips,
        budget,
    )?;
    let server_effects = server_capture
        .map(|capture| variables.finish_effects(capture))
        .unwrap_or_default();
    let clip_clocks = if advance_clocks || replay.is_some() {
        super::clock::prepare(
            &evaluator,
            &mut variables,
            Some(previous_clocks),
            &controllers,
            &mut weighted_clips,
            budget,
        )?
    } else {
        super::clock::sample(&evaluator, previous_clocks, &mut weighted_clips, budget)?;
        previous_clocks.clone()
    };
    if let Some(capture) = selection_capture {
        render_frame.as_mut().unwrap().selection_effects = variables.finish_effects(capture);
    }
    if let Some(frame) = render_frame.as_mut() {
        let inputs = super::render_frame::camera::active_pose_inputs(
            layout,
            state.presentation_expressions,
            &controllers,
            &weighted_clips,
        );
        frame.samples_camera_poses = state.samples_camera_poses && inputs.camera;
        frame.samples_swing_poses = state.samples_swing_poses && inputs.swing;
        frame.motion.clips.clone_from(&weighted_clips);
        frame.motion.journal = journal;
        frame.motion.server_effects = server_effects;
        // Camera frames resample endpoint weights, so camera rigs keep controller history.
        if samples_motion
            && (state.samples_camera_poses
                || state
                    .motion_sampling
                    .as_ref()
                    .is_some_and(|s| s.samples_clips()))
        {
            frame.motion.clocks.clone_from(&clip_clocks);
            frame.motion.controllers.clone_from(&controllers);
        }
    }
    let pose_capture = render_frame
        .as_ref()
        .filter(|frame| !frame.samples_camera_poses)
        .map(|_| variables.begin_effects());
    let local = sample_clips(
        &evaluator,
        &mut variables,
        &state.bones,
        &state.bone_names,
        &weighted_clips,
        budget,
    )?;
    if let Some(capture) = pose_capture {
        render_frame.as_mut().unwrap().retained_pose_effects = variables.finish_effects(capture);
    }
    let mut pose = vec![BoneTransform::default(); state.posed_bones().len()];
    state
        .compose_into(&local, &mut pose)
        .ok_or(EvalError::Invalid)?;
    // Initial publication needs a complete selection; later ticks can retain completed layers.
    let mut motion_layers = BTreeMap::new();
    let render = super::render::evaluate_render(
        &evaluator,
        &mut variables,
        super::render::RenderRig {
            binding: state.rig_binding,
            geometry: assets
                .rig_geometries()
                .get(state.geometry_binding)
                .ok_or(EvalError::Invalid)?
                .geometry,
            bone_names: state.posed_bone_names(),
            skeletons: &state.layer_skeletons,
        },
        budget,
    )
    .and_then(|layers| {
        let Some(mut layers) = layers else {
            return Ok(None);
        };
        let posed = super::render::pose_layers(
            &evaluator,
            &variables,
            &state.layer_skeletons,
            &weighted_clips,
            &mut layers,
            samples_motion.then_some(&mut motion_layers),
            budget,
        );
        if !state.initialized {
            posed?;
        }
        Ok(Some(layers))
    });
    let render = match render {
        Ok(layers) => layers,
        Err(error) if !state.initialized => return Err(error),
        Err(_) => None,
    };
    let skin_layers = super::skin_layers::evaluate(
        state,
        &evaluator,
        &variables,
        (&local, None),
        render.as_deref(),
    );
    if samples_motion && let Some(frame) = render_frame.as_mut() {
        frame.motion_poses = Some(super::render_frame::MotionPoses {
            previous: Vec::new(),
            previous_mask: Vec::new(),
            current: local,
            mask: state.motion_sampling.as_ref().unwrap().mask(
                assets,
                &state.bone_names,
                weighted_clips.iter().copied(),
                None,
            ),
        });
        frame.motion_layers = motion_layers
            .into_iter()
            .map(|(geometry, local)| {
                (
                    geometry,
                    super::render_frame::MotionPoses {
                        previous: Vec::new(),
                        previous_mask: Vec::new(),
                        current: local,
                        mask: state.motion_sampling.as_ref().unwrap().mask(
                            assets,
                            &state.layer_skeletons[&geometry].as_ref().unwrap().names,
                            weighted_clips.iter().copied(),
                            Some(geometry),
                        ),
                    },
                )
            })
            .collect();
    }
    Ok(EvaluatedState {
        pose,
        skin_layers,
        render,
        scale,
        held_item_scale,
        controllers,
        server_animations,
        clip_clocks,
        variables,
        render_frame,
    })
}

/// Refreshes the variables the client assigns every tick before `pre_animation`.
pub(super) fn apply_engine_variables(
    engine: &EngineSlots,
    variables: &mut MolangVariables,
    actor: &dyn AnimatedActor,
    context: &ActorTickContext,
    input: &ActorTickInput,
    motion: &MotionState,
) {
    let truth = |value: bool| if value { 1.0 } else { 0.0 };
    let flag = |bit| truth(query::actor_flag(actor, bit));
    let gliding = if query::actor_flag(actor, ActorFlag::Gliding.wire_value()) {
        let speed_squared = input
            .position_delta
            .iter()
            .map(|axis| axis * axis)
            .sum::<f32>();
        (speed_squared / GLIDING_SPEED_SQUARED_UNIT)
            .powi(3)
            .max(1.0)
    } else {
        1.0
    };
    variables.set(engine.attack_time, motion.attack_time());
    variables.set(engine.gliding_speed_value, gliding);
    variables.set(engine.is_holding_right, truth(context.main_hand.is_some()));
    variables.set(engine.is_holding_left, truth(context.off_hand.is_some()));
    variables.set(engine.is_sneaking, flag(ActorFlag::Sneaking.wire_value()));
    variables.set(
        engine.chest_layer_visible,
        truth(!query::wearing_elytra(context)),
    );
    variables.set(engine.is_blocking, flag(ActorFlag::Blocking.wire_value()));
    variables.set(
        engine.damage_nearby_mobs,
        flag(ActorFlag::DamageNearbyMobs.wire_value()),
    );
    let (startup, interval) = context
        .main_hand
        .as_ref()
        .and_then(|_| crate::consumables::consume_animation(context.main_hand_consumption))
        .map_or((0.0, 0.0), |_| {
            use_item_progress(input.item_use_ticks, context.main_hand_max_use_ticks)
        });
    variables.set(engine.use_item_startup_progress, startup);
    variables.set(engine.use_item_interval_progress, interval);
    variables.set(engine.swim_amount, input.swim_amount);
    variables.set(engine.left_arm_swim_amount, input.swim_amount);
    variables.set(engine.right_arm_swim_amount, input.swim_amount);
    variables.set(engine.has_target, truth(query::has_target(actor)));
    variables.set(engine.is_first_person, truth(context.is_local_first_person));
    variables.set(
        engine.player_x_rotation,
        if context.is_in_ui { 0.0 } else { input.pitch },
    );
    variables.set(engine.player_arm_height, input.arm_height);
    variables.set(
        engine.context_player_offhand_arm_height,
        input.off_hand_arm_height,
    );
    variables.set(
        engine.bob_animation,
        f32::from(context.view_bobbing.unwrap_or(true)),
    );
    if is_native_fish(actor) {
        // Vanilla publishes the fish animation phase before pack scripts.
        let [current, previous] = motion.fish_phase();
        variables.set(engine.fish_animation_amount, current);
        variables.set(engine.fish_animation_amount_previous, previous);
    }
    if let Some([base, pattern]) = query::tropical_fish_variables(actor) {
        variables.set(engine.tropical_fish_base, base);
        variables.set(engine.tropical_fish_pattern, pattern);
    }
    if actor.is_horse() {
        variables.set(engine.horse_stand_anim, motion.horse.stand_amount);
        variables.set(engine.horse_shake_tail, truth(motion.horse.shake_tail()));
        variables.set(
            engine.horse_open_mouth,
            truth(super::horse::mouth_open(actor)),
        );
    }
    for &(slot, offset, axis) in &engine.dragon_history {
        if let Some(frame) = actor.dragon_historical_frame(offset) {
            variables.set(Some(slot), frame[axis]);
        }
    }
}

/// Remaining use ticks above which the arm is still rising to the mouth.
const USE_ITEM_STARTUP_REMAINING: u32 = 25;
/// Remaining-tick period of one eat or drink bob.
const USE_ITEM_INTERVAL_TICKS: u32 = 4;

/// The `variable.use_item_startup_progress` and `variable.use_item_interval_progress` an eat or
/// drink use publishes on its `use_ticks`-th (one-based) tick of a `max_ticks` use; both are zero
/// while nothing is in use. An undeclared duration uses the default consumable duration.
pub(super) fn use_item_progress(use_ticks: u32, max_ticks: u32) -> (f32, f32) {
    if use_ticks == 0 {
        return (0.0, 0.0);
    }
    let max_ticks = if max_ticks == 0 {
        assets::item_use::DEFAULT_CONSUME_USE_TICKS
    } else {
        max_ticks
    };
    let remaining = max_ticks.saturating_sub(use_ticks - 1);
    if remaining >= USE_ITEM_STARTUP_REMAINING {
        let span = max_ticks.saturating_sub(USE_ITEM_STARTUP_REMAINING).max(1) as f32;
        let startup = 1.0 - (remaining - USE_ITEM_STARTUP_REMAINING) as f32 / span;
        return (startup, 0.0);
    }
    let interval = (remaining % USE_ITEM_INTERVAL_TICKS) as f32 / USE_ITEM_INTERVAL_TICKS as f32;
    (1.0, interval)
}

/// Uses the authored rotation factor when the pack leaves its item-specific factor unassigned.
pub(super) fn set_item_rotation_factor(
    engine: &evaluation::EngineSlots,
    variables: &mut MolangVariables,
) {
    if variables
        .get(engine.first_person_item_rotation_factor)
        .is_none()
        && let Some(factor) = variables.get(engine.first_person_rotation_factor)
    {
        variables.set(engine.first_person_item_rotation_factor, factor);
    }
}

/// Reads the native fish posture flag admitted with the actor's base class.
fn is_native_fish(actor: &dyn AnimatedActor) -> bool {
    actor.entity_fact().fish_animation
}

/// A clip to sample, its blend weight, and the animation tick its controller state began.
#[derive(Clone, Copy, Debug)]
pub(super) struct WeightedClip {
    pub(super) clip: usize,
    pub(super) weight: f32,
    pub(super) started_tick: u64,
    pub(super) clock: super::clock::Basis,
    /// Assigned once before posing, shared by every geometry this clip animates.
    pub(super) time: f32,
    pub(super) blend: Option<ControllerBlend>,
}

/// A clip on one side of a shortest-path controller blend, sampled at full weight.
#[derive(Clone, Copy, Debug)]
pub(super) struct ControllerBlend {
    /// The blending controller's slot; each controller's sides compose on their own.
    pub(super) controller: usize,
    pub(super) incoming: bool,
    /// Progress from the outgoing to the incoming state.
    pub(super) amount: f32,
}

fn blend_weight(
    evaluator: &Evaluator<'_>,
    variables: &mut MolangVariables,
    expression: Option<u32>,
    parent: f32,
    budget: &mut EvalBudget<'_>,
) -> Result<f32, EvalError> {
    let weight = match expression {
        Some(expression) => evaluator.number(expression as usize, variables, 0.0, budget)?,
        None => 1.0,
    };
    // A non-finite weight propagates, as in vanilla, and fails the pose closed.
    Ok(parent * weight)
}
/// Uses neutral owner rotations for view-following queries in the camera-space hand.
pub(super) fn evaluation_input(
    input: ActorTickInput,
    context: &ActorTickContext,
) -> ActorTickInput {
    if context.is_local_first_person {
        ActorTickInput {
            body_yaw: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            pitch: 0.0,
            ..input
        }
    } else {
        input
    }
}

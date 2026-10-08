use super::{query::FLAG_BABY, *};
use assets::EntityControllerAnimationTarget;

pub(super) mod selection;

#[cfg(test)]
#[path = "tick_cape_tests.rs"]
mod cape_tests;

/// Actor state beyond the snapshot that one tick's evaluation reads.
#[derive(Clone, Debug, Default)]
pub struct ActorTickContext {
    /// Frame fraction supplied only to scratch render-layer evaluation.
    pub frame_alpha: f32,
    /// Full elapsed visual interval; absent for an explicit single-tick evaluation.
    pub animation_elapsed_ticks: Option<u32>,
    pub is_riding: bool,
    /// Namespaced identifiers of the equipped main-hand and off-hand items.
    pub main_hand: Option<Arc<str>>,
    pub off_hand: Option<Arc<str>>,
    /// The main-hand stack's data value.
    pub main_hand_metadata: u32,
    /// A positive network identity distinguishes mutation from replacement of the held stack.
    pub main_hand_stack_id: Option<i32>,
    /// The selected hotbar slot, retained independently from the stack identity.
    pub main_hand_slot: u8,
    /// Current local Bedrock duration; zero retains the duration of a remote swing.
    pub bedrock_swing_ticks: i32,
    /// Current local Java swing length after haste and fatigue; remote swings use the default.
    pub java_swing_ticks: i32,
    /// A held crossbow is loaded.
    pub hand_charged: bool,
    /// Ticks the main-hand item can be used for, or 0 when unknown.
    pub main_hand_max_use_ticks: u32,
    /// Namespaced identifier of the actor being ridden.
    pub ridden: Option<Arc<str>>,
    pub has_rider: bool,
    pub has_player_rider: bool,
    /// The local player rendered from its own camera; selects the first-person render controller.
    pub is_local_first_person: bool,
    /// Local view-bobbing preference; other actor contexts keep the native default.
    pub view_bobbing: Option<bool>,
    /// The client's own player.
    pub is_local: bool,
    /// The client's own player is flying.
    pub is_flying: bool,
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
    pub skin_geometry: Option<Arc<protocol::SkinGeometrySource>>,
    /// The actor type's synced property definitions, in wire index order.
    pub(crate) properties: Option<Arc<[crate::actor_store::properties::PropertyDefinition]>>,
    /// Item-render query units and contexts differ from ordinary actor queries.
    pub(crate) attachable: Option<super::attachable::AttachableQueryContext>,
}

/// One worn armor stack as the armor queries read it.
#[derive(Clone, Debug)]
pub struct WornArmor {
    pub item: Arc<str>,
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
    actor: &ActorSnapshot,
    context: &ActorTickContext,
    reset_history: bool,
) {
    if reset_history && state.reset_pending {
        state.history.clear();
    }
    let previous_position = state
        .history
        .back()
        .map_or(actor.position, |input| input.position);
    let position_delta = std::array::from_fn(|axis| actor.position[axis] - previous_position[axis]);
    let motion = &mut state.motion;
    if context.bedrock_swing_ticks > 0 {
        motion.set_swing_duration(context.bedrock_swing_ticks);
    }
    motion.advance(&MotionInput {
        delta: position_delta,
        riding: context.is_riding,
        player: matches!(actor.kind, ActorKind::Player { .. }),
        yaw: actor.yaw,
        head_yaw: actor.head_yaw,
    });
    if is_native_fish(actor) {
        motion.advance_fish(actor.native_velocity());
    }
    if actor.is_horse() {
        motion
            .horse
            .advance(query::actor_flag(actor, query::FLAG_STANDING));
    }
    // Arrow orientation is entirely in animation.arrow.move's body bone. It is not
    // a mob: its interpolated body yaw is 0, while
    // query.target_y_rotation reads the actor's absolute rotation.
    if query::is_arrow(actor) {
        motion.body_yaw = 0.0;
        motion.previous_body_yaw = 0.0;
    }
    let baby_scale = if query::actor_flag(actor, FLAG_BABY) {
        BABY_MOVE_SPEED_SCALE
    } else {
        1.0
    };
    let item_use_ticks = if query::actor_flag(actor, query::FLAG_USING_ITEM) {
        state
            .history
            .back()
            .map_or(0, |input| input.item_use_ticks)
            .saturating_add(1)
    } else {
        0
    };
    let swim_target = if query::actor_flag(actor, query::FLAG_SWIMMING) {
        1.0
    } else {
        0.0
    };
    let swim_amount = state.history.back().map_or(swim_target, |input| {
        let previous = input.swim_amount;
        previous + (swim_target - previous).clamp(-SWIM_AMOUNT_STEP, SWIM_AMOUNT_STEP)
    });
    // The arm lowers while the held item differs from the equipped one, swaps it low, then rises.
    let arm_height = match state.history.back() {
        Some(previous) => advance_equip_height(
            &mut state.equipped_main,
            &context.main_hand,
            previous.arm_height,
        ),
        None => {
            state.equipped_main.clone_from(&context.main_hand);
            1.0
        }
    };
    state.off_hand_animation[0] = state.off_hand_animation[1];
    let off_hand_arm_height = advance_equip_height(
        &mut state.equipped_off,
        &context.off_hand,
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
        yaw: actor.yaw,
        local_swing: state.local_swing.map(|progress| progress.java),
        swing_ticks: if context.java_swing_ticks > 0 {
            context.java_swing_ticks
        } else {
            ACTOR_SWING_TICKS
        },
        held: &java_held,
        held_slot: context.main_hand_slot,
        riding: context.is_riding,
        vanilla_posture: swim_amount > 0.0
            || query::actor_flag(actor, query::FLAG_GLIDING)
            || query::actor_flag(actor, crate::actor_store::ACTOR_FLAG_CRAWLING)
            || query::actor_flag(actor, query::FLAG_EMOTING)
            || actor.is_sleeping(),
        position: actor.position,
        velocity: actor.native_velocity(),
        on_ground: actor.on_ground.unwrap_or(false),
        alive: !actor.status.dead
            && actor
                .attributes
                .get("minecraft:health")
                .is_none_or(|health| health.current > 0.0),
        sneaking: query::actor_flag(actor, query::FLAG_SNEAKING),
        flying: context.is_flying,
        local: context.is_local,
    });
    if state.history.len() == MAX_ACTOR_ACTION_HISTORY {
        state.history.pop_front();
    }
    let input = ActorTickInput {
        position: actor.position,
        position_delta,
        velocity: actor.velocity,
        on_ground: actor.on_ground.unwrap_or(false),
        body_yaw: motion.body_yaw,
        yaw: actor.yaw,
        head_yaw: actor.head_yaw,
        pitch: actor.pitch,
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

/// Identifier changes request the native lowering transition. Item-specific stack
/// equivalence/instant-update predicates need stack data beyond the current actor feed.
fn advance_equip_height(
    equipped: &mut Option<Arc<str>>,
    requested: &Option<Arc<str>>,
    previous_height: f32,
) -> f32 {
    let swapping = equipped != requested;
    let target = if swapping { 0.0 } else { 1.0 };
    let height =
        previous_height + (target - previous_height).clamp(-ARM_HEIGHT_STEP, ARM_HEIGHT_STEP);
    if swapping && height <= ARM_SWAP_HEIGHT {
        equipped.clone_from(requested);
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
    actor: &ActorSnapshot,
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
    let input = if context.is_local_first_person {
        ActorTickInput {
            body_yaw: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            pitch: 0.0,
            ..observed
        }
    } else {
        observed
    };
    let motion = state.motion;
    let evaluator = Evaluator {
        assets,
        layout,
        program: None,
        actor,
        input: &input,
        context,
        anim_tick,
        anim_time: None,
        life_tick,
        finished: (false, false),
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
        for &(name, value) in inheritance.overrides {
            variables.set(layout.named_slot(assets, name), value);
        }
    }
    if let Some(attachable) = context.attachable {
        variables.set_actor_reference(engine.context_owning_entity, actor.runtime_id);
        variables.set(
            engine.context_first_person,
            f32::from(attachable.first_person),
        );
        variables.set(engine.context_paperdoll, f32::from(attachable.is_paperdoll));
        variables.set_string(
            engine.context_item_slot,
            if attachable.off_hand {
                "off_hand"
            } else {
                "main_hand"
            },
        );
        variables.set(engine.attack_time, observed.attack_time);
    }
    variables.clear_temporaries();
    variables.clear(engine.first_person_item_rotation_factor);
    let mut render_frame = (state.samples_render_frames
        || (state.samples_swing_poses && state.local_swing.is_some()))
    .then(|| super::render_frame::FrameState {
        variables: variables.clone(),
        context: context.clone(),
        input,
        anim_tick,
        clips: Vec::new(),
    });
    if let Some(script) = rig.pre_animation {
        evaluator.run(script as usize, &mut variables, 0.0, budget)?;
    }
    // Authored scale scripts read the variables pre_animation just set.
    let scale = match rig.scale_expressions {
        None => None,
        Some(expressions) => {
            let mut scale = [1.0; 4];
            for (slot, expression) in scale.iter_mut().zip(expressions) {
                let value = evaluator.number(expression as usize, &mut variables, 1.0, budget)?;
                *slot = if value.is_finite() { value } else { 1.0 };
            }
            Some(scale)
        }
    };
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
    let reset_clocks = state
        .clip_clocks
        .iter()
        .filter(|((_, _, basis), _)| *basis == super::clock::Basis::Lifetime)
        .map(|(key, value)| (*key, *value))
        .collect();
    let previous_clocks = if reset {
        &reset_clocks
    } else {
        replay.map_or(&state.clip_clocks, |replay| &replay.clocks)
    };
    let mut weighted_clips = selection::select(
        &evaluator,
        &mut variables,
        &mut controllers,
        previous_clocks,
        state.geometry_binding,
        blink_controller,
        budget,
    )?;
    let mut server_animations = replay
        .map_or(&state.server_animations, |replay| &replay.server_animations)
        .clone();
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
    if (state.samples_camera_poses || (state.samples_swing_poses && state.local_swing.is_some()))
        && let Some(frame) = render_frame.as_mut()
    {
        frame.clips.clone_from(&weighted_clips);
    }
    let local = sample_clips(
        &evaluator,
        &mut variables,
        &state.bones,
        &state.bone_names,
        &weighted_clips,
        budget,
    )?;
    let pose = state.compose(&local).ok_or(EvalError::Invalid)?;
    // Render selection must not freeze the pose when it alone exceeds the budget.
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
    .ok()
    .map(|mut layers| {
        super::render::pose_layers(
            &evaluator,
            &variables,
            &state.layer_skeletons,
            &weighted_clips,
            &mut layers,
            budget,
        );
        layers
    });
    let skin_layers =
        super::skin_layers::evaluate(state, &evaluator, &variables, &local, render.as_deref());
    Ok(EvaluatedState {
        pose,
        skin_layers,
        render,
        scale,
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
    actor: &ActorSnapshot,
    context: &ActorTickContext,
    input: &ActorTickInput,
    motion: &MotionState,
) {
    let truth = |value: bool| if value { 1.0 } else { 0.0 };
    let flag = |bit| truth(query::actor_flag(actor, bit));
    let gliding = if query::actor_flag(actor, query::FLAG_GLIDING) {
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
    variables.set(engine.is_sneaking, flag(query::FLAG_SNEAKING));
    variables.set(
        engine.chest_layer_visible,
        truth(!query::wearing_elytra(context)),
    );
    variables.set(engine.is_blocking, flag(query::FLAG_BLOCKING));
    variables.set(
        engine.damage_nearby_mobs,
        flag(query::FLAG_DAMAGE_NEARBY_MOBS),
    );
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
    if let Some(state) = &actor.dragon_animation {
        let dead = actor.status.dead
            || actor
                .attributes
                .get("minecraft:health")
                .is_some_and(|health| health.current <= 0.0);
        for &(slot, offset, axis) in &engine.dragon_history {
            variables.set(Some(slot), state.historical_frame(offset, dead)[axis]);
        }
    }
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

fn is_native_fish(actor: &ActorSnapshot) -> bool {
    matches!(&actor.kind, ActorKind::Entity { identifier } if matches!(identifier.as_ref(),
        "minecraft:cod" | "minecraft:salmon" | "minecraft:pufferfish" | "minecraft:tropicalfish"))
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

/// Advances one controller and collects its active state's clips, descending into nested
/// controllers with the product of the enclosing blend weights.
struct ControllerWalk<'e, 'v, 'b, 'w> {
    evaluator: &'e Evaluator<'e>,
    variables: &'v mut MolangVariables,
    controllers: &'v mut [ControllerState],
    clip_clocks: &'v super::clock::ClipClocks,
    clips: &'v mut Vec<WeightedClip>,
    budget: &'b mut EvalBudget<'w>,
}

impl ControllerWalk<'_, '_, '_, '_> {
    fn evaluate(&mut self, controller: usize, weight: f32, depth: usize) -> Result<(), EvalError> {
        if depth >= assets::MAX_ENTITY_CONTROLLER_NESTING {
            return Err(EvalError::Invalid);
        }
        let assets = self.evaluator.assets;
        let slot = self
            .controllers
            .iter()
            .position(|runtime| runtime.controller == controller)
            .ok_or(EvalError::Invalid)?;
        self.budget.charge_work()?;
        let state = self.advance(slot)?;
        self.controllers[slot].active = true;
        let runtime = self.controllers[slot];
        if let Some((previous, started, began)) = runtime.blend_from {
            let definition = &assets.controllers()[controller];
            let previous = definition.first_state as usize + previous as usize;
            let source = &assets.controller_states()[previous];
            let elapsed = (self
                .evaluator
                .anim_tick
                .saturating_sub(runtime.entered_tick) as f32
                + self.frame_alpha()
                - began)
                .max(0.0)
                * ACTOR_TICK_DURATION.as_secs_f32();
            let amount = (elapsed / source.blend_transition.get()).clamp(0.0, 1.0);
            if amount < 1.0 {
                if source.blend_via_shortest_path {
                    let blend = |incoming| {
                        Some(ControllerBlend {
                            controller: slot,
                            incoming,
                            amount,
                        })
                    };
                    self.animations(previous, weight, depth, started, blend(false))?;
                    return self.animations(
                        state,
                        weight,
                        depth,
                        runtime.entered_tick,
                        blend(true),
                    );
                }
                // Other blends apply both weighted states straight onto the shared pose.
                self.animations(previous, weight * (1.0 - amount), depth, started, None)?;
                return self.animations(state, weight * amount, depth, runtime.entered_tick, None);
            }
            self.controllers[slot].blend_from = None;
        }
        self.animations(state, weight, depth, runtime.entered_tick, None)
    }

    fn frame_alpha(&self) -> f32 {
        self.evaluator
            .context
            .attachable
            .map_or(0.0, |input| input.frame_alpha)
    }

    /// Keeps outgoing and incoming clip channels together before composing the bone hierarchy.
    fn animations(
        &mut self,
        state: usize,
        weight: f32,
        depth: usize,
        started_tick: u64,
        blend: Option<ControllerBlend>,
    ) -> Result<(), EvalError> {
        for animation in state_animations(self.evaluator.assets, state)? {
            self.budget.charge_work()?;
            let weight = blend_weight(
                self.evaluator,
                self.variables,
                animation.weight,
                weight,
                self.budget,
            )?;
            if weight == 0.0 {
                continue;
            }
            match animation.target {
                EntityControllerAnimationTarget::Clip(clip) => self.clips.push(WeightedClip {
                    clip: clip as usize,
                    weight,
                    started_tick,
                    clock: super::clock::Basis::Controller,
                    time: 0.0,
                    blend,
                }),
                EntityControllerAnimationTarget::Controller(nested) => {
                    self.evaluate(nested as usize, weight, depth + 1)?
                }
            }
        }
        Ok(())
    }
}

fn state_animations(
    assets: &RuntimeEntityAssets,
    state: usize,
) -> Result<&[assets::EntityControllerAnimation], EvalError> {
    {
        let state = assets
            .controller_states()
            .get(state)
            .ok_or(EvalError::Invalid)?;
        let first = state.first_animation as usize;
        let end = first
            .checked_add(state.animation_count as usize)
            .ok_or(EvalError::Invalid)?;
        assets
            .controller_animations()
            .get(first..end)
            .ok_or(EvalError::Invalid)
    }
}

impl ControllerWalk<'_, '_, '_, '_> {
    /// Whether every and any clip of a state has played through once since it was entered.
    fn finished(&self, state: usize, entered_tick: u64) -> Result<(bool, bool), EvalError> {
        let elapsed = self.evaluator.anim_tick.saturating_sub(entered_tick) as f32
            * ACTOR_TICK_DURATION.as_secs_f32();
        let mut all = true;
        let mut any = false;
        for animation in state_animations(self.evaluator.assets, state)? {
            let EntityControllerAnimationTarget::Clip(index) = animation.target else {
                continue;
            };
            let clip = self
                .evaluator
                .assets
                .animation_clips()
                .get(index as usize)
                .ok_or(EvalError::Invalid)?;
            let done = if clip.anim_time_update.is_some() {
                self.clip_clocks
                    .get(&(
                        index as usize,
                        entered_tick,
                        super::clock::Basis::Controller,
                    ))
                    .is_some_and(|clock| clock.finished)
            } else {
                elapsed >= clip.length_seconds.get()
            };
            all &= done;
            any |= done;
        }
        Ok((all, any))
    }

    /// Takes at most the bounded number of transitions; returns the absolute state index.
    fn advance(&mut self, slot: usize) -> Result<usize, EvalError> {
        let assets = self.evaluator.assets;
        let runtime = self.controllers[slot];
        let controller = assets
            .controllers()
            .get(runtime.controller)
            .ok_or(EvalError::Invalid)?;
        let (mut current, mut entered_tick) = (runtime.state, runtime.entered_tick);
        loop {
            if current >= controller.state_count {
                return Err(EvalError::Invalid);
            }
            let state_index = controller.first_state as usize + current as usize;
            let state = assets
                .controller_states()
                .get(state_index)
                .ok_or(EvalError::Invalid)?;
            let first = state.first_transition as usize;
            let end = first
                .checked_add(state.transition_count as usize)
                .ok_or(EvalError::Invalid)?;
            let transitions = assets
                .controller_transitions()
                .get(first..end)
                .ok_or(EvalError::Invalid)?;
            let evaluator = Evaluator {
                finished: if transitions.is_empty() {
                    (false, false)
                } else {
                    self.finished(state_index, entered_tick)?
                },
                ..*self.evaluator
            };
            let mut target = None;
            for transition in transitions {
                self.budget.charge_work()?;
                let condition = evaluator.run(
                    transition.condition as usize,
                    self.variables,
                    0.0,
                    self.budget,
                )?;
                if condition.truthy() {
                    target = Some(transition.target_state);
                    break;
                }
            }
            let Some(target) = target.filter(|_| self.budget.take_transition()) else {
                self.controllers[slot].state = current;
                self.controllers[slot].entered_tick = entered_tick;
                return Ok(state_index);
            };
            if target >= controller.state_count {
                return Err(EvalError::Invalid);
            }
            if let Some(script) = state.on_exit {
                evaluator.run(script as usize, self.variables, 0.0, self.budget)?;
            }
            let worn = self
                .evaluator
                .context
                .attachable
                .is_some_and(|input| input.worn);
            let single_clip = |index| {
                state_animations(assets, index).is_ok_and(|animations| {
                    matches!(
                        animations,
                        [assets::EntityControllerAnimation {
                            target: EntityControllerAnimationTarget::Clip(_),
                            ..
                        }]
                    )
                })
            };
            self.controllers[slot].blend_from = (worn
                && state.blend_transition.get() > 0.0
                && single_clip(state_index)
                && single_clip(controller.first_state as usize + target as usize))
            .then_some((current, entered_tick, self.frame_alpha()));
            current = target;
            entered_tick = self.evaluator.anim_tick;
            let entered = assets
                .controller_states()
                .get(controller.first_state as usize + target as usize)
                .ok_or(EvalError::Invalid)?;
            if let Some(script) = entered.on_entry {
                evaluator.run(script as usize, self.variables, 0.0, self.budget)?;
            }
            if worn {
                self.controllers[slot].state = current;
                self.controllers[slot].entered_tick = entered_tick;
                return Ok(controller.first_state as usize + current as usize);
            }
        }
    }
}

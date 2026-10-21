use super::*;
use assets::EntityAnimationProperty;
use std::collections::BTreeSet;

mod masks;
use masks::ClipMasks;

pub(super) const TRANSLATION: u8 = 1;
pub(super) const ROTATION: u8 = 2;
pub(super) const SCALE: u8 = 4;
pub(super) const ROTATION_FRAME: u8 = 8;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MotionSampling {
    frame_queries: Option<Arc<[u32]>>,
    uses_swell: bool,
    uses_rotation: bool,
    uses_walk: bool,
    expressions: Box<[u32]>,
    weighted_symbols: Box<[u32]>,
    timed_symbols: Box<[u32]>,
    render_writes: bool,
    queries: Box<[u32]>,
    properties: bool,
    selection_effects: bool,
    rig_scale: bool,
    masks: Box<[ClipMasks]>,
}

/// Resamples every channel of `bones` from both endpoints.
pub(super) fn full_mask(bones: usize) -> Vec<[u8; 3]> {
    vec![[TRANSLATION | ROTATION | SCALE | ROTATION_FRAME; 3]; bones]
}

impl MotionSampling {
    /// Tracks sampled motion queries and their authored variable and channel dependencies.
    pub(crate) fn new(
        assets: &RuntimeEntityAssets,
        layout: &VariableLayout,
        rig: usize,
        geometry: usize,
        controllers: &[ControllerState],
        extra_clips: &[usize],
    ) -> Arc<Self> {
        let expressions =
            super::sampling::pose_expressions(assets, rig, geometry, controllers, extra_clips);
        let frame_queries =
            super::cache::Inputs::queries(assets, expressions.iter().copied()).query_plan();
        let uses_swell = expressions.iter().any(|&expression| {
            ops(assets, expression).iter().any(|op| match op {
                MolangOp::LoadQuery(symbol) => is_swell(assets, *symbol),
                MolangOp::CallQuery(call) => is_swell(assets, call.symbol),
                _ => false,
            })
        });
        let uses_rotation = expressions.iter().any(|&expression| {
            ops(assets, expression).iter().any(|op| match op {
                MolangOp::LoadQuery(symbol) => super::rotation::is_query(assets, *symbol),
                MolangOp::CallQuery(call) => super::rotation::is_query(assets, call.symbol),
                _ => false,
            })
        });
        let uses_walk = expressions.iter().any(|&expression| {
            ops(assets, expression).iter().any(|op| match op {
                MolangOp::LoadQuery(symbol) => super::rotation::is_walk_query(assets, *symbol),
                MolangOp::CallQuery(call) => super::rotation::is_walk_query(assets, call.symbol),
                _ => false,
            })
        });
        let mut queries = BTreeSet::new();
        let mut properties = false;
        for &expression in &expressions {
            for op in ops(assets, expression) {
                let symbol = match op {
                    MolangOp::LoadQuery(symbol) => Some(*symbol),
                    MolangOp::CallQuery(call) if call.arguments == 0 => Some(call.symbol),
                    MolangOp::CallQuery(call) => {
                        properties |= assets
                            .molang_symbols()
                            .get(call.symbol as usize)
                            .is_some_and(|s| {
                                assets::MolangQuery::from_name(&s.identifier)
                                    == Some(assets::MolangQuery::Property)
                            });
                        None
                    }
                    _ => None,
                };
                // Camera queries read each frame's live camera, never a frozen endpoint value.
                if let Some(symbol) = symbol
                    && !super::rotation::is_query(assets, symbol)
                    && !super::rotation::is_walk_query(assets, symbol)
                    && !super::camera::camera_symbol(assets, symbol, false)
                    && assets
                        .molang_symbols()
                        .get(symbol as usize)
                        .is_some_and(|s| {
                            !matches!(
                                assets::MolangQuery::from_name(&s.identifier),
                                Some(
                                    assets::MolangQuery::SwellAmount
                                        | assets::MolangQuery::FrameAlpha
                                        | assets::MolangQuery::AnimTime
                                        | assets::MolangQuery::AllAnimationsFinished
                                        | assets::MolangQuery::AnyAnimationFinished
                                        | assets::MolangQuery::StateTime
                                )
                            )
                        })
                {
                    queries.insert(symbol);
                }
            }
        }
        let mut variables = assets
            .molang_symbols()
            .iter()
            .enumerate()
            .filter_map(|(symbol, _)| {
                let symbol = symbol as u32;
                layout.symbol_inputs(symbol).swing.then_some(symbol)
            })
            .collect::<BTreeSet<_>>();
        let mut random = false;
        let (expressions, weighted_symbols) = loop {
            let before = (variables.len(), random);
            for &expression in &expressions {
                propagate(assets, expression, false, &mut variables, &mut random);
            }
            let dependent = dependent_expressions(assets, &expressions, &variables, random);
            let (weighted, controlled) =
                activation_dependencies(assets, geometry, controllers, &dependent);
            for expression in controlled {
                propagate(assets, expression, true, &mut variables, &mut random);
            }
            for clip in assets.animation_clips() {
                if !weighted.contains(&clip.symbol)
                    && !clip
                        .anim_time_update
                        .is_some_and(|time| dependent.contains(&time))
                {
                    continue;
                }
                for expression in clip_expressions(assets, clip) {
                    if expressions.contains(&expression) {
                        propagate(assets, expression, true, &mut variables, &mut random);
                    }
                }
            }
            for layer in assets.render_layers(rig) {
                let layer_expressions = super::sampling::render_layer_expressions(assets, layer);
                if layer_expressions
                    .gates
                    .iter()
                    .any(|gate| dependent.contains(gate))
                {
                    for expression in layer_expressions.expressions {
                        propagate(assets, expression, true, &mut variables, &mut random);
                    }
                }
            }
            if (variables.len(), random) == before {
                break (dependent, weighted);
            }
        };
        let selection_effects =
            super::sampling::selection_expressions(assets, geometry, controllers)
                .into_iter()
                .chain(
                    extra_clips
                        .iter()
                        .filter_map(|&clip| assets.animation_clips()[clip].anim_time_update),
                )
                .any(|expression| has_effects(assets, expression));
        let timed_symbols: BTreeSet<_> = assets
            .animation_clips()
            .iter()
            .filter(|clip| {
                clip.anim_time_update
                    .is_some_and(|expression| expressions.contains(&expression))
            })
            .map(|clip| clip.symbol)
            .collect();
        let render_writes = super::sampling::render_controller_expressions(assets, rig)
            .into_iter()
            .any(|expression| has_effects(assets, expression));
        let rig_scale = assets.rig_bindings()[rig]
            .scale_expressions
            .is_some_and(|scales| scales.iter().any(|scale| expressions.contains(scale)));
        let mut sampling = Self {
            frame_queries,
            uses_swell,
            uses_rotation,
            uses_walk,
            render_writes,
            queries: freeze(queries),
            properties,
            selection_effects,
            expressions: freeze(expressions),
            weighted_symbols: freeze(weighted_symbols),
            timed_symbols: freeze(timed_symbols),
            rig_scale,
            masks: Box::new([]),
        };
        sampling.masks = sampling.bind_masks(assets, rig, geometry, controllers, extra_clips);
        Arc::new(sampling)
    }

    /// Retained object and exact sorted-set payload, excluding the caller's Arc header.
    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        // Exhaustive matching makes new retained fields require an accounting review.
        let Self {
            frame_queries,
            expressions,
            weighted_symbols,
            timed_symbols,
            queries,
            uses_swell: _,
            uses_rotation: _,
            uses_walk: _,
            render_writes: _,
            properties: _,
            selection_effects: _,
            rig_scale: _,
            masks,
        } = self;
        let bytes = masks
            .iter()
            .try_fold(std::mem::size_of::<Self>(), |bytes, masks| {
                bytes.checked_add(masks.retained_bytes()?)
            })?;
        [expressions, weighted_symbols, timed_symbols, queries]
            .into_iter()
            .try_fold(bytes, |bytes, values| {
                bytes.checked_add(values.len().checked_mul(std::mem::size_of::<u32>())?)
            })
            .and_then(|bytes| {
                let extra = match frame_queries {
                    Some(queries) => queries
                        .len()
                        .checked_mul(std::mem::size_of::<u32>())?
                        .checked_add(2 * std::mem::size_of::<usize>())?,
                    None => 0,
                };
                bytes.checked_add(extra)
            })
    }

    /// Retains dependency analysis but restores the previous mask path for differential tests.
    #[cfg(test)]
    pub(crate) fn reference_masks(&self) -> Arc<Self> {
        Arc::new(Self {
            masks: Box::new([]),
            ..self.clone()
        })
    }

    /// Returns live-query dependencies analyzed with this exact rig and clip topology.
    pub(super) fn frame_queries(&self) -> Option<Arc<[u32]>> {
        self.frame_queries.clone()
    }

    pub(super) fn uses_swell(&self) -> bool {
        self.uses_swell
    }

    /// Rotation consumers sample independently of swelling and camera queries.
    pub(crate) fn uses_rotation(&self) -> bool {
        self.uses_rotation
    }

    /// Walk stride and speed consumers sample at the frame fraction.
    pub(crate) fn uses_walk(&self) -> bool {
        self.uses_walk
    }

    /// Whether a retained dependency changes the authored uniform or axis scale.
    pub(super) fn samples_rig_scale(&self) -> bool {
        self.rig_scale
    }

    /// Replaces only dependent scale slots, retaining the completed ordinary rig scale.
    pub(super) fn sampled_scale(
        &self,
        rig: &assets::EntityRigBinding,
        sampled: Option<[f32; 4]>,
        completed: Option<[f32; 4]>,
    ) -> Option<[f32; 4]> {
        let expressions = rig.scale_expressions?;
        let sampled = sampled?;
        let mut scale = completed.unwrap_or([rig.scale.get(), 1.0, 1.0, 1.0]);
        let mut changed = false;
        for (slot, expression) in expressions.into_iter().enumerate() {
            if self.expressions.binary_search(&expression).is_ok() {
                scale[slot] = sampled[slot];
                changed = true;
            }
        }
        changed.then_some(scale)
    }

    /// Ordinary query values belong to the completed motion endpoint.
    pub(crate) fn freeze_queries(
        &self,
        evaluator: &evaluation::Evaluator<'_>,
    ) -> Vec<(u32, MolangValue)> {
        self.queries
            .iter()
            .map(|&symbol| (symbol, evaluator.query(symbol, &[])))
            .collect()
    }

    /// Argument-based property queries also require the retained actor maps.
    pub(crate) fn samples_properties(&self) -> bool {
        self.properties
    }

    /// Authored render assignments must retain each selected geometry endpoint's inputs.
    pub(super) fn samples_render_writes(&self) -> bool {
        self.render_writes
    }

    pub(crate) fn samples_clips(&self) -> bool {
        self.selection_effects
            || !self.weighted_symbols.is_empty()
            || !self.timed_symbols.is_empty()
    }

    pub(super) fn samples_time(&self, assets: &RuntimeEntityAssets, clip: usize) -> bool {
        assets
            .animation_clips()
            .get(clip)
            .is_some_and(|clip| self.timed_symbols.binary_search(&clip.symbol).is_ok())
    }

    /// Builds an uncached union for constructor analysis and unbound dynamic clips.
    fn mask_unbound(
        &self,
        assets: &RuntimeEntityAssets,
        names: &[Box<str>],
        clips: impl IntoIterator<Item = tick::WeightedClip>,
        geometry: Option<u32>,
    ) -> Vec<[u8; 3]> {
        let mut mask = vec![[0; 3]; names.len()];
        let mut channel_bones = pose::ChannelBones::new(names);
        for weighted in clips {
            let weighted = match geometry {
                Some(geometry) => match render::clip_for_layer(assets, weighted, geometry) {
                    Some(weighted) => weighted,
                    None => continue,
                },
                None => weighted,
            };
            let Some(clip) = assets.animation_clips().get(weighted.clip) else {
                continue;
            };
            let weighted = self.weighted_symbols.binary_search(&clip.symbol).is_ok()
                || self.timed_symbols.binary_search(&clip.symbol).is_ok();
            let first = clip.first_channel as usize;
            for channel in &assets.animation_channels()[first..first + clip.channel_count as usize]
            {
                let index = channel_bones.resolve(channel);
                let Some(bone) = index.and_then(|index| mask.get_mut(index)) else {
                    continue;
                };
                let bit = match channel.property {
                    EntityAnimationProperty::Translation => TRANSLATION,
                    EntityAnimationProperty::Rotation => ROTATION,
                    EntityAnimationProperty::Scale => SCALE,
                };
                if weighted && clip.override_previous {
                    bone.fill(TRANSLATION | ROTATION | SCALE | ROTATION_FRAME);
                } else if weighted && channel.rotation_relative_to_entity {
                    bone[0] |= ROTATION_FRAME;
                }
                let first = channel.first_keyframe as usize;
                for key in
                    &assets.animation_keyframes()[first..first + channel.keyframe_count as usize]
                {
                    for (axis, expression) in key.expressions.iter().enumerate() {
                        let neutral = if channel.property == EntityAnimationProperty::Scale {
                            pose::LocalDelta::default().scale[axis]
                        } else {
                            0.0
                        };
                        // Each test is pure, so set bits and cheap weighted reads go first.
                        if (bone[axis] & bit) == 0
                            && ((weighted
                                && (expression.is_some() || key.value[axis].get() != neutral))
                                || expression.is_some_and(|expression| {
                                    self.expressions.binary_search(&expression).is_ok()
                                }))
                        {
                            bone[axis] |= bit;
                        }
                    }
                }
            }
        }
        mask
    }
}

fn controller_dependencies(
    assets: &RuntimeEntityAssets,
    root: usize,
    symbols: &mut BTreeSet<u32>,
    scripts: &mut BTreeSet<u32>,
) {
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    while let Some(index) = pending.pop() {
        if !seen.insert(index) {
            continue;
        }
        let controller = &assets.controllers()[index];
        let first = controller.first_state as usize;
        for state in &assets.controller_states()[first..first + usize::from(controller.state_count)]
        {
            scripts.extend(state.on_entry);
            scripts.extend(state.on_exit);
            let first = state.first_transition as usize;
            scripts.extend(
                assets.controller_transitions()[first..first + state.transition_count as usize]
                    .iter()
                    .map(|transition| transition.condition),
            );
            let first = state.first_animation as usize;
            for animation in
                &assets.controller_animations()[first..first + usize::from(state.animation_count)]
            {
                scripts.extend(animation.weight);
                match animation.target {
                    assets::EntityControllerAnimationTarget::Clip(clip) => {
                        symbols.insert(assets.animation_clips()[clip as usize].symbol);
                    }
                    assets::EntityControllerAnimationTarget::Controller(controller) => {
                        pending.push(controller as usize)
                    }
                }
            }
        }
    }
}

fn ops(assets: &RuntimeEntityAssets, expression: u32) -> &[MolangOp] {
    let Some(expression) = assets.molang_expressions().get(expression as usize) else {
        return &[];
    };
    let first = expression.first_op as usize;
    &assets.molang_ops()[first..first + usize::from(expression.op_count)]
}

fn is_swell(assets: &RuntimeEntityAssets, symbol: u32) -> bool {
    assets
        .molang_symbols()
        .get(symbol as usize)
        .is_some_and(|symbol| symbol.identifier.as_ref() == "query.swell_amount")
}

/// Fresh presentation inputs share the authored dependency closure.
fn is_presentation_query(assets: &RuntimeEntityAssets, symbol: u32) -> bool {
    is_swell(assets, symbol)
        || super::rotation::is_query(assets, symbol)
        || super::rotation::is_walk_query(assets, symbol)
        || assets
            .molang_symbols()
            .get(symbol as usize)
            .is_some_and(|symbol| symbol.identifier.as_ref() == "query.frame_alpha")
}

/// Propagates query dependencies through authored assignments without running the script.
fn propagate(
    assets: &RuntimeEntityAssets,
    expression: u32,
    controlled: bool,
    variables: &mut BTreeSet<u32>,
    random: &mut bool,
) {
    let ops = ops(assets, expression);
    let mut states: Vec<Option<(Vec<bool>, bool)>> = vec![None; ops.len() + 1];
    let controls = if controlled {
        vec![(ops.len(), true)]
    } else {
        Vec::new()
    };
    let mut pending = vec![(0, Vec::new(), controls)];
    while let Some((pc, mut stack, mut controls)) = pending.pop() {
        controls.retain(|(end, _)| pc < *end);
        let mut control = controls.iter().any(|(_, dependency)| *dependency);
        if let Some((existing, old_control)) = &mut states[pc] {
            let mut changed = !*old_control && control;
            control |= *old_control;
            *old_control = control;
            for (old, value) in existing.iter_mut().zip(&stack) {
                changed |= !*old && *value;
                *old |= *value;
            }
            if !changed {
                continue;
            }
            stack.clone_from(existing);
        } else {
            states[pc] = Some((stack.clone(), control));
        }
        let Some(op) = ops.get(pc) else { continue };
        let mut pop = |count: usize| {
            let start = stack.len().saturating_sub(count);
            let dependency = stack[start..].iter().any(|value| *value);
            stack.truncate(start);
            dependency
        };
        match *op {
            MolangOp::Push(_) | MolangOp::PushString(_) => stack.push(control),
            MolangOp::LoadThis => stack.push(true),
            MolangOp::LoadQuery(symbol) => {
                stack.push(control || is_presentation_query(assets, symbol))
            }
            MolangOp::LoadVariable(symbol) => stack.push(control || variables.contains(&symbol)),
            MolangOp::CallQuery(call) => {
                let dependency = pop(usize::from(call.arguments));
                stack.push(control || dependency || is_presentation_query(assets, call.symbol));
            }
            MolangOp::StoreVariable(symbol) => {
                if pop(1) || control {
                    variables.insert(symbol);
                }
            }
            MolangOp::Pop | MolangOp::Return => {
                pop(1);
            }
            MolangOp::Call(function) => {
                let dependency = pop(function.arity());
                if function.is_random() {
                    *random |= control || dependency;
                }
                stack.push(control || dependency || (function.is_random() && *random));
            }
            MolangOp::Add
            | MolangOp::Subtract
            | MolangOp::Multiply
            | MolangOp::Divide
            | MolangOp::Less
            | MolangOp::LessEqual
            | MolangOp::Greater
            | MolangOp::GreaterEqual
            | MolangOp::Equal
            | MolangOp::NotEqual => {
                let dependency = pop(2);
                stack.push(control || dependency);
            }
            MolangOp::Jump(target) | MolangOp::LoopBreak(target) => {
                pending.push((usize::from(target), stack, controls));
                continue;
            }
            MolangOp::JumpIfFalse(target)
            | MolangOp::JumpIfTrue(target)
            | MolangOp::LoopStart(target) => {
                let dependency = pop(1) || control;
                let target = usize::from(target);
                let end = ops[pc + 1..target].iter().fold(target, |end, op| {
                    if let MolangOp::Jump(next) | MolangOp::LoopBreak(next) = op {
                        end.max(usize::from(*next))
                    } else {
                        end
                    }
                });
                // A returning branch controls whether the rest of the script executes.
                let end = if ops[pc + 1..end].contains(&MolangOp::Return) {
                    ops.len()
                } else {
                    end
                };
                controls.push((end, dependency));
                pending.push((target, stack.clone(), controls.clone()));
            }
            MolangOp::Coalesce(branch) => {
                let dependency = control || variables.contains(&branch.symbol);
                controls.push((usize::from(branch.target), dependency));
                let mut assigned = stack.clone();
                assigned.push(dependency);
                pending.push((usize::from(branch.target), assigned, controls.clone()));
            }
            MolangOp::LoopNext(target)
            | MolangOp::ForEachNext(assets::MolangBranch { target, .. }) => {
                pending.push((usize::from(target), stack.clone(), controls.clone()));
            }
            MolangOp::ForEachStart(branch) => {
                pop(1);
                pending.push((usize::from(branch.target), stack, controls));
                continue;
            }
            MolangOp::Arrow(target) => {
                let dependency = pop(1);
                let mut absent = stack.clone();
                absent.push(dependency || control);
                pending.push((usize::from(target), absent, controls.clone()));
            }
            MolangOp::SelectCollection(_) | MolangOp::Negate | MolangOp::Not | MolangOp::Truthy => {
            }
        }
        if !matches!(op, MolangOp::Return) {
            pending.push((pc + 1, stack, controls));
        }
    }
}

fn has_effects(assets: &RuntimeEntityAssets, expression: u32) -> bool {
    ops(assets, expression).iter().any(|op| {
        matches!(op, MolangOp::StoreVariable(_))
            || matches!(op, MolangOp::Call(function) if function.is_random())
    })
}

fn dependent_expressions(
    assets: &RuntimeEntityAssets,
    expressions: &[u32],
    variables: &BTreeSet<u32>,
    random: bool,
) -> BTreeSet<u32> {
    expressions
        .iter()
        .copied()
        .filter(|&expression| {
            ops(assets, expression).iter().any(|op| match op {
                MolangOp::LoadQuery(symbol) => is_presentation_query(assets, *symbol),
                MolangOp::CallQuery(call) => is_presentation_query(assets, call.symbol),
                MolangOp::LoadVariable(symbol) => variables.contains(symbol),
                MolangOp::LoadThis => true,
                MolangOp::Call(function) => random && function.is_random(),
                MolangOp::Coalesce(branch) => variables.contains(&branch.symbol),
                _ => false,
            })
        })
        .collect()
}

fn activation_dependencies(
    assets: &RuntimeEntityAssets,
    geometry: usize,
    controllers: &[ControllerState],
    expressions: &BTreeSet<u32>,
) -> (BTreeSet<u32>, BTreeSet<u32>) {
    let mut weighted_symbols = BTreeSet::new();
    let mut scripts = BTreeSet::new();
    let geometry = &assets.rig_geometries()[geometry];
    let first = geometry.first_animation as usize;
    for binding in &assets.rig_animations()[first..first + usize::from(geometry.animation_count)] {
        if binding
            .weight
            .is_some_and(|weight| expressions.contains(&weight))
        {
            weighted_symbols.insert(assets.animation_clips()[binding.clip as usize].symbol);
        }
    }
    let first = geometry.first_controller as usize;
    for binding in &assets.rig_controllers()[first..first + usize::from(geometry.controller_count)]
    {
        if binding
            .weight
            .is_some_and(|weight| expressions.contains(&weight))
        {
            controller_dependencies(
                assets,
                binding.controller as usize,
                &mut weighted_symbols,
                &mut scripts,
            );
        }
    }
    for runtime in controllers {
        let controller = &assets.controllers()[runtime.controller];
        let first = controller.first_state as usize;
        for state in &assets.controller_states()[first..first + usize::from(controller.state_count)]
        {
            let first = state.first_animation as usize;
            for animation in
                &assets.controller_animations()[first..first + usize::from(state.animation_count)]
            {
                if !animation
                    .weight
                    .is_some_and(|weight| expressions.contains(&weight))
                {
                    continue;
                }
                match animation.target {
                    assets::EntityControllerAnimationTarget::Clip(clip) => {
                        weighted_symbols.insert(assets.animation_clips()[clip as usize].symbol);
                    }
                    assets::EntityControllerAnimationTarget::Controller(controller) => {
                        controller_dependencies(
                            assets,
                            controller as usize,
                            &mut weighted_symbols,
                            &mut scripts,
                        )
                    }
                }
            }
        }
    }
    (weighted_symbols, scripts)
}

fn clip_expressions(assets: &RuntimeEntityAssets, clip: &assets::EntityAnimationClip) -> Vec<u32> {
    let mut expressions = Vec::new();
    expressions.extend(clip.anim_time_update);
    let first = clip.first_channel as usize;
    for channel in &assets.animation_channels()[first..first + clip.channel_count as usize] {
        let first = channel.first_keyframe as usize;
        for key in &assets.animation_keyframes()[first..first + channel.keyframe_count as usize] {
            expressions.extend(key.expressions.into_iter().flatten());
        }
    }
    expressions
}

/// Freezes the complete ordered set without capacity slack or changing membership.
fn freeze(values: BTreeSet<u32>) -> Box<[u32]> {
    values.into_iter().collect()
}

#[cfg(test)]
mod frozen_tests {
    use super::*;

    #[test]
    fn frozen_sets_preserve_empty_duplicate_extreme_and_borrowed_membership() {
        for values in [vec![], vec![0, u32::MAX, 0, 7, 7, 1]] {
            let expected: BTreeSet<u32> = values.into_iter().collect();
            let frozen = freeze(expected.clone());
            assert_eq!(
                frozen.as_ref(),
                expected.iter().copied().collect::<Vec<_>>()
            );
            for probe in [0, 1, 2, 7, u32::MAX - 1, u32::MAX] {
                assert_eq!(
                    frozen.binary_search(&probe).is_ok(),
                    expected.contains(&probe)
                );
            }
        }
    }
}

use super::*;
use assets::EntityAnimationProperty;
use std::collections::BTreeSet;

pub(super) const TRANSLATION: u8 = 1;
pub(super) const ROTATION: u8 = 2;
pub(super) const SCALE: u8 = 4;
pub(super) const ROTATION_FRAME: u8 = 8;

#[derive(Debug)]
pub(in crate::actor_animation) struct SwellSampling {
    expressions: BTreeSet<u32>,
    weighted_symbols: BTreeSet<u32>,
    timed_symbols: BTreeSet<u32>,
    render_writes: bool,
    queries: BTreeSet<u32>,
    properties: bool,
    selection_effects: bool,
}

impl SwellSampling {
    pub(in crate::actor_animation) fn new(
        assets: &RuntimeEntityAssets,
        rig: usize,
        geometry: usize,
        controllers: &[ControllerState],
        extra_clips: &[usize],
    ) -> Option<Arc<Self>> {
        let expressions =
            super::sampling::pose_expressions(assets, rig, geometry, controllers, extra_clips);
        if !expressions.iter().any(|&expression| {
            ops(assets, expression).iter().any(|op| match op {
                MolangOp::LoadQuery(symbol) => is_swell(assets, *symbol),
                MolangOp::CallQuery(call) => is_swell(assets, call.symbol),
                _ => false,
            })
        }) {
            return None;
        }
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
                            .is_some_and(|s| s.identifier.as_ref() == "query.property");
                        None
                    }
                    _ => None,
                };
                if let Some(symbol) = symbol
                    && assets
                        .molang_symbols()
                        .get(symbol as usize)
                        .is_some_and(|s| {
                            !matches!(
                                s.identifier.as_ref(),
                                "query.swell_amount"
                                    | "query.frame_alpha"
                                    | "query.anim_time"
                                    | "query.all_animations_finished"
                                    | "query.any_animation_finished"
                            )
                        })
                {
                    queries.insert(symbol);
                }
            }
        }
        let mut variables = BTreeSet::new();
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
        let timed_symbols = assets
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
        Some(Arc::new(Self {
            render_writes,
            queries,
            properties,
            selection_effects,
            expressions,
            weighted_symbols,
            timed_symbols,
        }))
    }

    /// Ordinary query values belong to the completed motion endpoint.
    pub(in crate::actor_animation) fn freeze_queries(
        &self,
        evaluator: &evaluation::Evaluator<'_>,
    ) -> Vec<(u32, MolangValue)> {
        self.queries
            .iter()
            .map(|&symbol| (symbol, evaluator.query(symbol, &[])))
            .collect()
    }

    /// Argument-based property queries also require the retained actor maps.
    pub(in crate::actor_animation) fn samples_properties(&self) -> bool {
        self.properties
    }

    /// Authored render assignments must retain each selected geometry endpoint's inputs.
    pub(super) fn samples_render_writes(&self) -> bool {
        self.render_writes
    }

    pub(in crate::actor_animation) fn samples_clips(&self) -> bool {
        self.selection_effects
            || !self.weighted_symbols.is_empty()
            || !self.timed_symbols.is_empty()
    }

    pub(super) fn samples_time(&self, assets: &RuntimeEntityAssets, clip: usize) -> bool {
        assets
            .animation_clips()
            .get(clip)
            .is_some_and(|clip| self.timed_symbols.contains(&clip.symbol))
    }

    pub(in crate::actor_animation) fn mask(
        &self,
        assets: &RuntimeEntityAssets,
        names: &[Box<str>],
        clips: impl IntoIterator<Item = tick::WeightedClip>,
        geometry: Option<u32>,
    ) -> Vec<[u8; 3]> {
        let mut mask = vec![[0; 3]; names.len()];
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
            let first = clip.first_channel as usize;
            for channel in &assets.animation_channels()[first..first + clip.channel_count as usize]
            {
                let index = match &channel.bone_name {
                    Some(name) => names.iter().position(|candidate| candidate == name),
                    None => Some(channel.bone as usize),
                };
                let Some(bone) = index.and_then(|index| mask.get_mut(index)) else {
                    continue;
                };
                let bit = match channel.property {
                    EntityAnimationProperty::Translation => TRANSLATION,
                    EntityAnimationProperty::Rotation => ROTATION,
                    EntityAnimationProperty::Scale => SCALE,
                };
                let weighted = self.weighted_symbols.contains(&clip.symbol)
                    || self.timed_symbols.contains(&clip.symbol);
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
                        if expression
                            .is_some_and(|expression| self.expressions.contains(&expression))
                            || (weighted
                                && (expression.is_some() || key.value[axis].get() != neutral))
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
                    if let MolangOp::Jump(next) = op {
                        end.max(usize::from(*next))
                    } else {
                        end
                    }
                });
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

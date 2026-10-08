use super::*;
use assets::EntityAnimationProperty;
use std::collections::BTreeSet;

pub(super) const TRANSLATION: u8 = 1;
pub(super) const ROTATION: u8 = 2;
pub(super) const SCALE: u8 = 4;

#[derive(Debug)]
pub(in crate::actor_animation) struct SwellSampling {
    expressions: BTreeSet<u32>,
}

impl SwellSampling {
    pub(in crate::actor_animation) fn new(
        assets: &RuntimeEntityAssets,
        rig: usize,
        geometry: usize,
        controllers: &[ControllerState],
    ) -> Option<Arc<Self>> {
        let expressions = super::sampling::pose_expressions(assets, rig, geometry, controllers);
        if !expressions.iter().any(|&expression| {
            ops(assets, expression).iter().any(|op| match op {
                MolangOp::LoadQuery(symbol) => is_swell(assets, *symbol),
                MolangOp::CallQuery(call) => is_swell(assets, call.symbol),
                _ => false,
            })
        }) {
            return None;
        }
        let mut variables = BTreeSet::new();
        loop {
            let before = variables.len();
            for &expression in &expressions {
                propagate(assets, expression, &mut variables);
            }
            if variables.len() == before {
                break;
            }
        }
        let expressions = expressions
            .into_iter()
            .filter(|&expression| {
                ops(assets, expression).iter().any(|op| match op {
                    MolangOp::LoadQuery(symbol) => is_swell(assets, *symbol),
                    MolangOp::CallQuery(call) => is_swell(assets, call.symbol),
                    MolangOp::LoadVariable(symbol) => variables.contains(symbol),
                    MolangOp::Coalesce(branch) => variables.contains(&branch.symbol),
                    _ => false,
                })
            })
            .collect::<BTreeSet<_>>();
        (!expressions.is_empty()).then(|| Arc::new(Self { expressions }))
    }

    pub(in crate::actor_animation) fn mask(
        &self,
        assets: &RuntimeEntityAssets,
        names: &[Box<str>],
        clips: &[tick::WeightedClip],
    ) -> Vec<[u8; 3]> {
        let mut mask = vec![[0; 3]; names.len()];
        for weighted in clips {
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
                let first = channel.first_keyframe as usize;
                for key in
                    &assets.animation_keyframes()[first..first + channel.keyframe_count as usize]
                {
                    for (axis, expression) in key.expressions.iter().enumerate() {
                        if expression
                            .is_some_and(|expression| self.expressions.contains(&expression))
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

/// Propagates query dependencies through authored assignments without running the script.
fn propagate(assets: &RuntimeEntityAssets, expression: u32, variables: &mut BTreeSet<u32>) {
    let ops = ops(assets, expression);
    let mut states: Vec<Option<(Vec<bool>, bool)>> = vec![None; ops.len() + 1];
    let mut pending = vec![(0, Vec::new(), Vec::<(usize, bool)>::new())];
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
            MolangOp::Push(_) | MolangOp::PushString(_) | MolangOp::LoadThis => stack.push(control),
            MolangOp::LoadQuery(symbol) => stack.push(control || is_swell(assets, symbol)),
            MolangOp::LoadVariable(symbol) => stack.push(control || variables.contains(&symbol)),
            MolangOp::CallQuery(call) => {
                let dependency = pop(usize::from(call.arguments));
                stack.push(control || dependency || is_swell(assets, call.symbol));
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
                stack.push(control || dependency);
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
                let mut assigned = stack.clone();
                assigned.push(control || variables.contains(&branch.symbol));
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

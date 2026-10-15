//! Reuses only frame-independent controller results with explicitly bounded inputs.
use super::*;
use assets::entity::EngineVariable as V;
use assets::{MolangFunction, MolangOp};
use protocol_ids::ActorFlag;

#[derive(Debug, Eq, PartialEq)]
enum Value {
    Number(u32),
    String(Arc<str>),
    Actor(u64),
}

impl Value {
    /// Retains value type and exact floating-point bits for inherited input comparisons.
    fn from_molang(value: &evaluation::MolangValue) -> Self {
        match value {
            evaluation::MolangValue::Number(value) => Self::Number(value.to_bits()),
            evaluation::MolangValue::String(value) => Self::String(Arc::clone(value)),
            evaluation::MolangValue::ActorReference(value) => Self::Actor(*value),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct Inputs {
    first_person: bool,
    off_hand: bool,
    paperdoll: bool,
    worn: bool,
    blocking: bool,
    target: bool,
    elapsed: Option<u32>,
    duration: u32,
    off_hand_duration: u32,
    frame: u32,
    enchanted: Value,
}

impl Inputs {
    /// Captures every external value admitted by the static-expression proof.
    pub(super) fn new(
        owner: &dyn AnimatedActor,
        rig: &ActorRigSnapshot<'_>,
        input: AttachableAnimationInput<'_>,
    ) -> Self {
        let inherited = rig.animation_variables;
        let enchanted = input
            .owner_variables
            .iter()
            .rev()
            .find(|(name, _)| *name == V::IsEnchanted.name())
            .map(|(_, value)| Value::Number(value.to_bits()))
            .or_else(|| {
                inherited
                    .value(V::IsEnchanted.name())
                    .map(|value| Value::from_molang(&value))
            })
            .unwrap_or(Value::Number(0.0f32.to_bits()));
        Self {
            first_person: input.first_person,
            off_hand: input.off_hand,
            paperdoll: input.is_paperdoll,
            worn: input.worn,
            blocking: query::actor_flag(owner, ActorFlag::Blocking.wire_value()),
            target: query::has_target(owner),
            elapsed: input.use_elapsed_ticks,
            duration: input.max_use_ticks,
            off_hand_duration: inherited
                .owner_context()
                .map_or(0, |context| context.off_hand_max_use_ticks),
            frame: input.animation_frame,
            enchanted,
        }
    }
}

#[derive(Debug)]
pub(super) struct StaticDraw {
    pub(super) input: Inputs,
    pub(super) owner_names: Vec<Box<str>>,
}

/// Allows only expressions whose external reads are represented by `Inputs`.
pub(crate) fn expression_is_static(
    assets: &RuntimeEntityAssets,
    expression: usize,
    preparation: bool,
) -> bool {
    let Some(expression) = assets.molang_expressions().get(expression) else {
        return false;
    };
    let first = expression.first_op as usize;
    let Some(ops) = assets
        .molang_ops()
        .get(first..first + expression.op_count as usize)
    else {
        return false;
    };
    if preparation && !writes_are_unconditional(ops) {
        return false;
    }
    let symbol = |index: u32| {
        assets
            .molang_symbols()
            .get(index as usize)
            .map(|symbol| &*symbol.identifier)
    };
    let context = |name: &str| {
        matches!(
            name,
            "context.is_first_person" | "context.item_slot" | "context.is_paperdoll"
        )
    };
    ops.iter().all(|op| match *op {
        MolangOp::LoadVariable(index) => symbol(index)
            .is_some_and(|name| context(name) || (!preparation && name == V::IsEnchanted.name())),
        MolangOp::StoreVariable(index) => {
            preparation
                && symbol(index).is_some_and(|name| {
                    name.starts_with("variable.") && name != V::IsEnchanted.name()
                })
        }
        MolangOp::LoadQuery(index)
        | MolangOp::CallQuery(assets::MolangCall { symbol: index, .. }) => {
            preparation
                || symbol(index).is_some_and(|name| {
                    matches!(
                        assets::MolangQuery::from_name(name),
                        Some(
                            assets::MolangQuery::Blocking
                                | assets::MolangQuery::HasTarget
                                | assets::MolangQuery::IsUsingItem
                                | assets::MolangQuery::MainHandItemUseDuration
                                | assets::MolangQuery::ItemRemainingUseDuration
                                | assets::MolangQuery::MainHandItemMaxDuration
                                | assets::MolangQuery::GetAnimationFrame
                        )
                    )
                })
        }
        MolangOp::Call(
            MolangFunction::Random
            | MolangFunction::RandomInteger
            | MolangFunction::DieRoll
            | MolangFunction::DieRollInteger,
        )
        | MolangOp::Coalesce(_)
        | MolangOp::ForEachStart(_)
        | MolangOp::ForEachNext(_)
        | MolangOp::Arrow(_)
        | MolangOp::LoopStart(_)
        | MolangOp::LoopNext(_)
        | MolangOp::LoopBreak(_) => false,
        _ => true,
    })
}

/// A later dependent pose must refresh every preparation write before reading it.
/// Forward branches may select assignment values but may not skip the assignments.
fn writes_are_unconditional(ops: &[MolangOp]) -> bool {
    ops.iter().enumerate().all(|(index, op)| {
        let skipped = match *op {
            MolangOp::Jump(target)
            | MolangOp::JumpIfFalse(target)
            | MolangOp::JumpIfTrue(target) => {
                let target = target as usize;
                if target <= index || target > ops.len() {
                    return false;
                }
                &ops[index + 1..target]
            }
            MolangOp::Return => &ops[index + 1..],
            _ => return true,
        };
        !skipped
            .iter()
            .any(|op| matches!(op, MolangOp::StoreVariable(_)))
    })
}

/// Static caching never pauses direct clocks, blends, or state-changing controller scripts.
pub(super) fn eligible(state: &ActorRigState, previous: &[ControllerState]) -> bool {
    state.clip_clocks.is_empty()
        && state.server_animations.is_empty()
        && state.controllers.len() == previous.len()
        && state
            .controllers
            .iter()
            .zip(previous)
            .all(|(current, previous)| {
                current.controller == previous.controller
                    && current.state == previous.state
                    && current.entered_tick == previous.entered_tick
                    && current.blend_from.is_none()
            })
}

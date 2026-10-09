//! Reuses only frame-independent controller results with explicitly bounded inputs.
use super::*;
use assets::{MolangFunction, MolangOp};

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
    frame: u32,
    enchanted: Value,
}

impl Inputs {
    /// Captures every external value admitted by the static-expression proof.
    pub(super) fn new(
        owner: &ActorSnapshot,
        rig: &ActorRigSnapshot<'_>,
        input: AttachableAnimationInput<'_>,
    ) -> Self {
        let inherited = rig.animation_variables;
        let enchanted = input
            .owner_variables
            .iter()
            .rev()
            .find(|(name, _)| *name == "variable.is_enchanted")
            .map(|(_, value)| Value::Number(value.to_bits()))
            .or_else(|| {
                inherited
                    .value("variable.is_enchanted")
                    .map(Value::from_molang)
            })
            .unwrap_or(Value::Number(0.0f32.to_bits()));
        Self {
            first_person: input.first_person,
            off_hand: input.off_hand,
            paperdoll: input.is_paperdoll,
            worn: input.worn,
            blocking: query::actor_flag(owner, query::FLAG_BLOCKING),
            target: query::has_target(owner),
            elapsed: input.use_elapsed_ticks,
            duration: input.max_use_ticks,
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
pub(in crate::actor_animation) fn expression_is_static(
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
            .is_some_and(|name| context(name) || (!preparation && name == "variable.is_enchanted")),
        MolangOp::StoreVariable(index) => {
            preparation
                && symbol(index).is_some_and(|name| {
                    name.starts_with("variable.") && name != "variable.is_enchanted"
                })
        }
        MolangOp::LoadQuery(index)
        | MolangOp::CallQuery(assets::MolangCall { symbol: index, .. }) => {
            preparation
                || symbol(index).is_some_and(|name| {
                    matches!(
                        name,
                        "query.blocking"
                            | "query.has_target"
                            | "query.is_using_item"
                            | "query.main_hand_item_use_duration"
                            | "query.item_remaining_use_duration"
                            | "query.main_hand_item_max_duration"
                            | "query.get_animation_frame"
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

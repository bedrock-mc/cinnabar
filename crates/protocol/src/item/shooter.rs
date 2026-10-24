//! `minecraft:shooter` facts that decide whether using a server item starts a held draw.

use crate::nbt_tree::Nbt;

/// The draw behaviour a shooter declares; the server still owns projectiles and ammunition.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ItemShooter {
    /// A positive finite `max_draw_duration`; otherwise using the item fires at once.
    pub draws: bool,
    /// `charge_on_draw` loads the item like a crossbow instead of firing on release.
    pub charge_on_draw: bool,
    /// Declared ammunition entries make the draw depend on inventory contents.
    pub ammunition: bool,
}

impl ItemShooter {
    /// Whether using the item always starts a held draw that a release ends.
    pub const fn starts_held_draw(self) -> bool {
        self.draws && !self.charge_on_draw && !self.ammunition
    }
}

/// Reads a shooter compound, treating missing fields as their declared defaults.
pub(super) fn parse_shooter(component: Option<&Nbt>) -> Option<ItemShooter> {
    let component = component.filter(|component| matches!(component, Nbt::Compound(_)))?;
    let flag = |name| {
        component
            .field(name)
            .and_then(Nbt::number)
            .is_some_and(|value| value != 0.0)
    };
    Some(ItemShooter {
        draws: component
            .field("max_draw_duration")
            .and_then(Nbt::number)
            .is_some_and(|seconds| seconds.is_finite() && seconds > 0.0),
        charge_on_draw: flag("charge_on_draw"),
        ammunition: !component.list("ammunition").is_empty(),
    })
}

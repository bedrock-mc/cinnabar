//! Held-item eligibility for component interactions, including explicit unknown state.

use serde::{Deserialize, Serialize};

/// A verified resolver can use component facts to rule out an offered interaction.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum ActorUseResolver {
    #[default]
    Unresolved,
    Components,
    PreventDefault,
}

/// An offering may be ruled out without claiming that an unresolved callback succeeds.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OfferedUse {
    Absent,
    #[default]
    Unresolved,
}

/// Component groups retain unknown activation rather than becoming active base components.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum UseSelector {
    Base,
    UnresolvedComponentGroup { group: u32 },
}

/// Supported necessary conditions; unsupported tests cannot establish absence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum UseCondition {
    Unresolved,
    Always,
    Never,
    /// The interacting player's main-hand item.
    HeldItem {
        identifier: Box<str>,
        metadata: Option<u32>,
    },
    All(Box<[Self]>),
    Any(Box<[Self]>),
    Not(Box<Self>),
    /// The item in either of the interacting player's hands.
    EitherHandItem {
        identifier: Box<str>,
        metadata: Option<u32>,
    },
    /// A main-hand spawn egg, the only item class that places actors.
    HeldActorPlacer,
    /// Any equipped item of the interacting player carries this enchantment id.
    EquipmentEnchantment {
        enchantment: u8,
    },
}

/// One possible offering and its actual component owner.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentUse {
    pub selector: UseSelector,
    pub component: Box<str>,
    pub condition: UseCondition,
}

/// A missing behavior definition supplies no negative evidence.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum EntityUseAdmission {
    #[default]
    Unresolved,
    Components {
        source: u32,
        offerings: Box<[ComponentUse]>,
    },
}

/// One hand of the interacting player as offering conditions read it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandItem<'a> {
    /// Unobserved or server-modified contents, which no condition can rule out.
    Unknown,
    Empty,
    Item {
        identifier: &'a str,
        metadata: u32,
    },
}

/// Both hands of the interacting player and the enchantments across its equipment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeldItems<'a> {
    pub main_hand: HandItem<'a>,
    pub offhand: HandItem<'a>,
    /// Bit `id` is set when a hand or armor item carries enchantment `id`; `None` while any
    /// of those slots is unobserved.
    pub equipment_enchantments: Option<u64>,
}

/// Resolves a filter's snake-case enchantment name, such as `fire_aspect`, to its wire id.
#[must_use]
pub fn enchantment_id(name: &str) -> Option<u8> {
    let mut letters = name.strip_prefix("minecraft:").unwrap_or(name).chars();
    let first = letters.next()?.to_ascii_uppercase();
    let variant: String = std::iter::once(first)
        .chain(letters.filter(|letter| *letter != '_'))
        .collect();
    protocol_ids::Enchantment::from_name(&variant)
        .map(protocol_ids::Enchantment::wire_value)
        .filter(|id| *id < protocol_ids::Enchantment::Numenchantments.wire_value())
}

impl HandItem<'_> {
    /// Compares the item identity; a wildcard auxiliary value on either side matches any variant.
    fn matches(self, expected: &str, aux: Option<u32>) -> Option<bool> {
        let (identifier, metadata) = match self {
            Self::Unknown => return None,
            Self::Empty => return Some(false),
            Self::Item {
                identifier,
                metadata,
            } => (identifier, metadata),
        };
        if identifier != expected {
            return Some(false);
        }
        let Some(aux) = aux else {
            return Some(true);
        };
        let aux = u16::try_from(aux).ok()?;
        let metadata = u16::try_from(metadata).ok()?;
        Some(
            aux == metadata
                || aux == protocol_ids::ITEM_AUXILIARY_WILDCARD
                || metadata == protocol_ids::ITEM_AUXILIARY_WILDCARD,
        )
    }

    /// Spawn eggs are the retail actor placers; any namespace keeps the suffix conservative.
    fn places_actors(self) -> Option<bool> {
        match self {
            Self::Unknown => None,
            Self::Empty => Some(false),
            Self::Item { identifier, .. } => {
                Some(identifier.ends_with("_spawn_egg") || identifier == "minecraft:spawn_egg")
            }
        }
    }
}

impl UseCondition {
    /// Evaluates only necessary held-item conditions; unknown dependencies stay unknown.
    pub fn permits(&self, held: &HeldItems<'_>) -> Option<bool> {
        match self {
            Self::Unresolved => None,
            Self::Always => Some(true),
            Self::Never => Some(false),
            Self::HeldItem {
                identifier,
                metadata,
            } => held.main_hand.matches(identifier, *metadata),
            Self::EitherHandItem {
                identifier,
                metadata,
            } => match (
                held.main_hand.matches(identifier, *metadata),
                held.offhand.matches(identifier, *metadata),
            ) {
                (Some(true), _) | (_, Some(true)) => Some(true),
                (Some(false), Some(false)) => Some(false),
                _ => None,
            },
            Self::HeldActorPlacer => held.main_hand.places_actors(),
            Self::EquipmentEnchantment { enchantment } => held
                .equipment_enchantments
                .map(|mask| mask & (1 << enchantment) != 0),
            Self::Not(condition) => condition.permits(held).map(|value| !value),
            Self::All(conditions) => {
                let mut unknown = false;
                for condition in conditions {
                    match condition.permits(held) {
                        Some(false) => return Some(false),
                        None => unknown = true,
                        Some(true) => {}
                    }
                }
                (!unknown).then_some(true)
            }
            Self::Any(conditions) => {
                let mut unknown = false;
                for condition in conditions {
                    match condition.permits(held) {
                        Some(true) => return Some(true),
                        None => unknown = true,
                        Some(false) => {}
                    }
                }
                (!unknown).then_some(false)
            }
        }
    }
}

impl EntityUseAdmission {
    /// Rules out all component offerings without inferring component-group activation.
    /// A matching condition retains unknown cooldown, activation and callback eligibility.
    pub fn for_held(&self, held: &HeldItems<'_>) -> OfferedUse {
        let Self::Components { offerings, .. } = self else {
            return OfferedUse::Unresolved;
        };
        if offerings
            .iter()
            .all(|offering| offering.condition.permits(held) == Some(false))
        {
            OfferedUse::Absent
        } else {
            OfferedUse::Unresolved
        }
    }
}

/// Checks source ownership and necessary conditions before retaining a carrier.
pub(super) fn validate(compiled: &super::CompiledEntityAssets) -> Result<(), crate::AssetError> {
    for entry in &compiled.interaction_traits {
        let EntityUseAdmission::Components { source, offerings } = &entry.offered_use else {
            continue;
        };
        if compiled.sources.get(*source as usize).is_none_or(|source| {
            !source.path.starts_with("behavior/entities/") || !source.path.ends_with(".json")
        }) {
            return Err(super::invalid(
                "interaction offering has no behavior source",
            ));
        }
        for offering in offerings {
            if !offering.component.contains(':') || !condition_valid(&offering.condition) {
                return Err(super::invalid("invalid compiled component interaction"));
            }
        }
    }
    Ok(())
}

/// Rejects malformed item identities without changing an unsupported condition into a match.
fn condition_valid(condition: &UseCondition) -> bool {
    match condition {
        UseCondition::HeldItem { identifier, .. }
        | UseCondition::EitherHandItem { identifier, .. } => identifier
            .split_once(':')
            .is_some_and(|(namespace, name)| !namespace.is_empty() && !name.is_empty()),
        UseCondition::All(conditions) | UseCondition::Any(conditions) => {
            conditions.iter().all(condition_valid)
        }
        UseCondition::Not(condition) => condition_valid(condition),
        UseCondition::EquipmentEnchantment { enchantment } => {
            *enchantment < protocol_ids::Enchantment::Numenchantments.wire_value()
        }
        _ => true,
    }
}

impl super::RuntimeEntityAssets {
    /// Borrows the immutable offering descriptor at actor-use admission.
    pub fn offered_use_for(&self, identifier: &str) -> Option<&EntityUseAdmission> {
        self.interaction_traits
            .binary_search_by(|entry| entry.identifier.as_ref().cmp(identifier))
            .ok()
            .map(|index| &self.interaction_traits[index].offered_use)
    }
}

#[cfg(test)]
mod tests;

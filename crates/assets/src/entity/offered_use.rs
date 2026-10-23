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
    HeldItem {
        identifier: Box<str>,
        metadata: Option<u32>,
    },
    All(Box<[Self]>),
    Any(Box<[Self]>),
    Not(Box<Self>),
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

impl UseCondition {
    /// Evaluates only necessary held-item conditions; unknown dependencies stay unknown.
    pub fn permits(&self, identifier: Option<&str>, metadata: u32) -> Option<bool> {
        match self {
            Self::Unresolved => None,
            Self::Always => Some(true),
            Self::Never => Some(false),
            Self::HeldItem {
                identifier: expected,
                metadata: aux,
            } => identifier.and_then(|identifier| {
                if identifier != expected.as_ref() {
                    return Some(false);
                }
                let Some(aux) = aux else {
                    return Some(true);
                };
                let aux = u16::try_from(*aux).ok()?;
                let metadata = u16::try_from(metadata).ok()?;
                Some(
                    aux == metadata
                        || aux == protocol_ids::ITEM_AUXILIARY_WILDCARD
                        || metadata == protocol_ids::ITEM_AUXILIARY_WILDCARD,
                )
            }),
            Self::Not(condition) => condition.permits(identifier, metadata).map(|value| !value),
            Self::All(conditions) => {
                let mut unknown = false;
                for condition in conditions {
                    match condition.permits(identifier, metadata) {
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
                    match condition.permits(identifier, metadata) {
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
    pub fn for_throwable(&self, identifier: Option<&str>, metadata: u32) -> OfferedUse {
        let Self::Components { offerings, .. } = self else {
            return OfferedUse::Unresolved;
        };
        if offerings
            .iter()
            .all(|offering| offering.condition.permits(identifier, metadata) == Some(false))
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
        UseCondition::HeldItem { identifier, .. } => identifier
            .split_once(':')
            .is_some_and(|(namespace, name)| !namespace.is_empty() && !name.is_empty()),
        UseCondition::All(conditions) | UseCondition::Any(conditions) => {
            conditions.iter().all(condition_valid)
        }
        UseCondition::Not(condition) => condition_valid(condition),
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

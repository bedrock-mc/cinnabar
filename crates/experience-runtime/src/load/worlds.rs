//! Validation of definitions before the adapter opens worlds.
use crate::{host::cinnabar::experience_server::types as wit, protocol as p};
use anyhow::{Result, ensure};
use std::collections::HashSet;

/// Checks stable names owned by one mod; never accepts filesystem path syntax.
pub(crate) fn owned_name(id: &str, owner: &str) -> bool {
    id.strip_prefix(owner)
        .and_then(|s| s.strip_prefix(':'))
        .is_some_and(super::is_name)
}

/// Validates definition and startup references before returning any of them to the adapter.
pub(crate) fn validate(
    owner: &str,
    registration: wit::WorldRegistration,
) -> Result<p::WorldRegistration> {
    let mut names = HashSet::new();
    let mut definitions = Vec::new();
    for def in registration.definitions {
        ensure!(
            owned_name(&def.id.value, owner) && names.insert(def.id.value.clone()),
            "invalid or duplicate world definition"
        );
        definitions.push(p::WorldDefinition {
            id: def.id.value,
            settings: p::WorldSettings {
                generator: match def.settings.generator {
                    wit::Generator::Empty => p::Generator::Empty,
                    wit::Generator::Flat => p::Generator::Flat,
                },
                dimension: match def.settings.dimension {
                    wit::DimensionType::Overworld => p::DimensionType::Overworld,
                    wit::DimensionType::Nether => p::DimensionType::Nether,
                    wit::DimensionType::End => p::DimensionType::End,
                },
                persistent: def.settings.persistent,
            },
        });
    }
    let mut instances = HashSet::new();
    let mut startup = Vec::new();
    for instance in registration.startup {
        ensure!(
            owned_name(&instance.id.value, owner) && instances.insert(instance.id.value.clone()),
            "invalid or duplicate startup instance"
        );
        ensure!(
            names.contains(&instance.definition.value),
            "startup instance has no registered definition"
        );
        startup.push(p::WorldInstance {
            id: instance.id.value,
            definition: instance.definition.value,
        });
    }
    Ok(p::WorldRegistration {
        definitions,
        startup,
    })
}

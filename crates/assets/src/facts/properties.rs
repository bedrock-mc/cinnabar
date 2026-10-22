//! Typed state values decoded once when a block record is admitted.

use super::BlockProperty;
use crate::AssetError;
use crate::registry::RegistryRecord;

/// A pinned property's allowed scalar values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyDomain {
    Bool,
    Int(&'static [i32]),
    String(&'static [&'static str]),
}

/// Returns the values `block`'s own states admit for `key`.
pub fn block_property_domain(block: &str, key: BlockProperty) -> PropertyDomain {
    narrowed_domain(super::property_domains_generated::block_domains(block), key)
}

/// Prefers a block's narrower domain over the vocabulary-wide one.
pub(crate) fn narrowed_domain(
    narrowed: &[(BlockProperty, PropertyDomain)],
    key: BlockProperty,
) -> PropertyDomain {
    narrowed
        .iter()
        .find(|(narrowed, _)| *narrowed == key)
        .map_or(key.domain(), |(_, domain)| *domain)
}

/// A state scalar, preserving the property's domain rather than its JSON envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateValue {
    Bool(bool),
    Int(i32),
    String(Box<str>),
}

impl RegistryRecord {
    /// Reads an admitted state value by numeric property key without parsing JSON.
    pub fn property(&self, key: BlockProperty) -> Option<&StateValue> {
        self.properties
            .binary_search_by_key(&key, |(key, _)| *key)
            .ok()
            .map(|index| &self.properties[index].1)
    }
}

/// Decodes recognized keys; unknown well-formed keys remain in the canonical state.
pub(crate) fn decode(state: &str) -> Result<Box<[(BlockProperty, StateValue)]>, AssetError> {
    let fields: std::collections::BTreeMap<String, serde_json::Value> =
        serde_json::from_str(state).map_err(|_| AssetError::InvalidRegistryFlags(0xff))?;
    let mut properties = Vec::with_capacity(fields.len());
    for (name, envelope) in fields {
        let Some(key) = BlockProperty::from_name(&name) else {
            tracing::debug!(property = name, "skipping unknown registry property");
            continue;
        };
        let Some(kind) = envelope["type"].as_str() else {
            return Err(AssetError::InvalidRegistryFlags(0xff));
        };
        let value = &envelope["value"];
        let parsed = match key.domain() {
            PropertyDomain::Bool if matches!(kind, "byte" | "bool") => value
                .as_bool()
                .or_else(|| value.as_u64().filter(|v| *v <= 1).map(|v| v != 0))
                .map(StateValue::Bool),
            PropertyDomain::Int(domain) if kind == "int" => value
                .as_i64()
                .and_then(|v| i32::try_from(v).ok())
                .filter(|v| domain.contains(v))
                .map(StateValue::Int),
            PropertyDomain::String(domain) if kind == "string" => value
                .as_str()
                .filter(|v| domain.contains(v))
                .map(|v| StateValue::String(v.into())),
            _ => None,
        };
        properties.push((key, parsed.ok_or(AssetError::InvalidRegistryFlags(0xff))?));
    }
    properties.sort_by_key(|(key, _)| *key);
    Ok(properties.into_boxed_slice())
}

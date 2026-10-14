//! Named attachable resources retained for native item presentation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{AssetError, invalid, validate_identifier};

/// Authored aliases; geometry names alone do not establish that a model is stock.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EquipmentResources {
    pub geometry: BTreeMap<Box<str>, Box<str>>,
    pub textures: BTreeMap<Box<str>, Box<str>>,
}

impl EquipmentResources {
    /// Bounds both alias tables and validates each alias and resource identifier.
    pub(super) fn validate(&self) -> Result<(), AssetError> {
        for table in [&self.geometry, &self.textures] {
            if table.len() > super::MAX_EQUIPMENT_BINDINGS {
                return Err(invalid("too many equipment resource aliases"));
            }
            for (alias, identifier) in table {
                validate_identifier(alias)?;
                validate_identifier(identifier)?;
            }
        }
        Ok(())
    }
}

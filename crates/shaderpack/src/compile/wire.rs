//! Transfers modules without duplicating the validation data derived from them.

use super::CompiledStage;
use crate::programs::StageKind;
use serde::{Deserialize, Deserializer, de::Error};

impl<'de> Deserialize<'de> for CompiledStage {
    /// Revalidates a received module before exposing its derived usage and type information.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Stage {
            stage: StageKind,
            glsl: String,
            module: naga::Module,
        }

        let Stage {
            stage,
            glsl,
            module,
        } = Stage::deserialize(deserializer)?;
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .map_err(|error| D::Error::custom(super::error_chain(&error)))?;
        Ok(Self {
            stage,
            glsl,
            module,
            info,
        })
    }
}

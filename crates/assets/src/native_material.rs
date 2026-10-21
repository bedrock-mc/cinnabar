/// Solid and liquid material facts, independent of visual and collision geometry.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize,
)]
#[repr(u8)]
pub enum NativeMaterialFacts {
    #[default]
    Unresolved = 0,
    NonSolidNonLiquid = 1,
    Solid = 2,
    Liquid = 3,
    SolidAndLiquid = 4,
    /// Preserves an existing admission whose complete material binding is unverified.
    ProvisionalNonSolidNonLiquid = 5,
}

impl NativeMaterialFacts {
    /// Decodes only defined material dispositions from a checked carrier.
    pub(crate) fn from_raw(value: u8) -> Result<Self, crate::AssetError> {
        match value {
            0 => Ok(Self::Unresolved),
            1 => Ok(Self::NonSolidNonLiquid),
            2 => Ok(Self::Solid),
            3 => Ok(Self::Liquid),
            4 => Ok(Self::SolidAndLiquid),
            5 => Ok(Self::ProvisionalNonSolidNonLiquid),
            _ => Err(crate::AssetError::InvalidCompiledAssets {
                detail: "visual has unknown native material facts".into(),
            }),
        }
    }
}

/// The compiler's partial material source; unlisted identities remain unresolved.
pub const NATIVE_MATERIAL_SOURCE_BYTES: &[u8] = include_bytes!("../data/native-materials.json");

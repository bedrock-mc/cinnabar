use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum CloudQuality {
    Low,
    Medium,
    #[default]
    High,
    Ultra,
}

impl CloudQuality {
    pub const ALL: [Self; 4] = [Self::Low, Self::Medium, Self::High, Self::Ultra];
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum PrecipitationQuality {
    Off,
    Low,
    #[default]
    High,
}

impl PrecipitationQuality {
    pub const ALL: [Self; 3] = [Self::Off, Self::Low, Self::High];
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentQualitySettings {
    pub clouds: CloudQuality,
    pub precipitation: PrecipitationQuality,
}

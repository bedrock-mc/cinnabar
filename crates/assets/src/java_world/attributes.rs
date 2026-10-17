//! Typed visual attributes and renderer-facing values.

use serde_json::Value;

/// Scalars and gamma-space colours share a fixed-size representation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttributeValue(pub [f32; 4]);

impl AttributeValue {
    /// Reads a scalar without changing its units.
    pub const fn scalar(self) -> f32 {
        self.0[0]
    }

    /// Returns the RGB channels of an evaluated colour.
    pub const fn rgb(self) -> [f32; 3] {
        [self.0[0], self.0[1], self.0[2]]
    }

    /// Interpolates scalar or gamma-space colour channels.
    pub fn mix(self, other: Self, amount: f32) -> Self {
        Self(std::array::from_fn(|i| {
            self.0[i] * (1.0 - amount) + other.0[i] * amount
        }))
    }
}

macro_rules! attributes {
    ($($name:ident => ($key:literal, $value:expr)),+ $(,)?) => {
        /// Inputs supported by the pinned Java world renderer.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Attribute { $($name),+ }
        impl Attribute {
            pub const ALL: &[Self] = &[$(Self::$name),+];
            /// Returns the namespaced resource key.
            pub const fn name(self) -> &'static str { match self { $(Self::$name => concat!("minecraft:visual/", $key)),+ } }
            /// Supplies an inherited base value before resource overlays.
            pub const fn base(self) -> AttributeValue { match self { $(Self::$name => AttributeValue($value)),+ } }
        }
    };
}

attributes! {
    FogColour => ("fog_color", [0.0, 0.0, 0.0, 1.0]),
    FogStart => ("fog_start_distance", [0.0; 4]),
    FogEnd => ("fog_end_distance", [1024.0; 4]),
    SkyFogEnd => ("sky_fog_end_distance", [512.0; 4]),
    CloudFogEnd => ("cloud_fog_end_distance", [2048.0; 4]),
    WaterFogColour => ("water_fog_color", [5.0 / 255.0, 5.0 / 255.0, 51.0 / 255.0, 1.0]),
    WaterFogStart => ("water_fog_start_distance", [-8.0; 4]),
    WaterFogEnd => ("water_fog_end_distance", [96.0; 4]),
    SkyColour => ("sky_color", [0.0, 0.0, 0.0, 1.0]),
    Sunrise => ("sunrise_sunset_color", [0.0; 4]),
    CloudColour => ("cloud_color", [0.0; 4]),
    CloudHeight => ("cloud_height", [192.33; 4]),
    SunAngle => ("sun_angle", [0.0; 4]),
    MoonAngle => ("moon_angle", [0.0; 4]),
    StarAngle => ("star_angle", [0.0; 4]),
    Stars => ("star_brightness", [0.0; 4]),
    BlockTint => ("block_light_tint", [1.0, 216.0 / 255.0, 140.0 / 255.0, 1.0]),
    SkyLightColour => ("sky_light_color", [1.0; 4]),
    SkyLightFactor => ("sky_light_factor", [1.0; 4]),
    NightVisionColour => ("night_vision_color", [0.6, 0.6, 0.6, 1.0]),
    Ambient => ("ambient_light_color", [0.0, 0.0, 0.0, 1.0]),
}

/// Sparse overlays preserve inheritance until dimension, biome and time are composed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JavaEnvironment([(Option<AttributeValue>, Modifier); Attribute::ALL.len()]);

/// Operations admitted by the selected visual resources.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Modifier {
    Replace,
    Multiply,
    Maximum,
}

impl Modifier {
    /// Rejects unsupported operations instead of silently using a different appearance.
    pub(super) fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.unwrap_or("override") {
            "override" => Ok(Self::Replace),
            "multiply" => Ok(Self::Multiply),
            "maximum" => Ok(Self::Maximum),
            _ => Err("unsupported Java visual attribute modifier".into()),
        }
    }

    /// Composes a sampled layer with the value beneath it.
    pub(super) fn apply(self, base: AttributeValue, value: AttributeValue) -> AttributeValue {
        match self {
            Self::Replace => value,
            Self::Multiply => AttributeValue(std::array::from_fn(|i| base.0[i] * value.0[i])),
            Self::Maximum => AttributeValue(std::array::from_fn(|i| base.0[i].max(value.0[i]))),
        }
    }
}

impl Default for JavaEnvironment {
    /// Leaves every input inherited from its lower layer.
    fn default() -> Self {
        Self([(None, Modifier::Replace); Attribute::ALL.len()])
    }
}

impl JavaEnvironment {
    /// Looks up an evaluated input, retaining its base when no layer supplies it.
    pub fn value(self, attribute: Attribute) -> AttributeValue {
        let (value, modifier) = self.0[attribute as usize];
        value.map_or_else(
            || attribute.base(),
            |value| modifier.apply(attribute.base(), value),
        )
    }

    /// Assigns one validated input.
    pub fn set(&mut self, attribute: Attribute, value: AttributeValue) {
        self.0[attribute as usize] = (Some(value), Modifier::Replace);
    }

    /// Applies authored values while preserving unspecified lower-layer inputs.
    pub(crate) fn overlay(&mut self, overlay: &Self) {
        for &attribute in Attribute::ALL {
            if let (Some(value), modifier) = overlay.0[attribute as usize] {
                self.set(attribute, modifier.apply(self.value(attribute), value));
            }
        }
    }

    /// Reads supported visual inputs and ignores unrelated gameplay attributes.
    pub(crate) fn parse(value: &Value) -> Result<Self, String> {
        let mut environment = Self::default();
        for &attribute in Attribute::ALL {
            if let Some(value) = value.get(attribute.name()) {
                let modifier = Modifier::parse(value.get("modifier").and_then(Value::as_str))?;
                let argument = value.get("argument").unwrap_or(value);
                environment.0[attribute as usize] = (Some(parse_value(argument)?), modifier);
            }
        }
        Ok(environment)
    }
}

/// Accepts bounded finite scalars and RGB/ARGB literals from a visual resource.
pub(super) fn parse_value(value: &Value) -> Result<AttributeValue, String> {
    if let Some(value) = value
        .as_f64()
        .filter(|value| value.is_finite() && value.abs() <= 1.0e9)
    {
        return Ok(AttributeValue([value as f32; 4]));
    }
    let colour = value
        .as_str()
        .and_then(|value| value.strip_prefix('#'))
        .filter(|value| matches!(value.len(), 6 | 8))
        .ok_or_else(|| "invalid Java visual attribute value".to_owned())?;
    let packed = u32::from_str_radix(colour, 16).map_err(|_| "invalid Java visual colour")?;
    Ok(AttributeValue([
        ((packed >> 16) & 255) as f32 / 255.0,
        ((packed >> 8) & 255) as f32 / 255.0,
        (packed & 255) as f32 / 255.0,
        if colour.len() == 8 {
            (packed >> 24) as f32 / 255.0
        } else {
            1.0
        },
    ]))
}

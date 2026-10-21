//! The pack's effect directives from `shaders.properties`.

use super::Effect;
use shaderpack::ShaderProperties;

/// `clouds=` in `shaders.properties`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CloudSetting {
    /// The player's cloud setting.
    #[default]
    Default,
    Off,
    /// Flat cloud layers.
    Fast,
    /// Cuboid clouds.
    Fancy,
}

/// When particles draw relative to the deferred passes (Iris's `ParticleRenderingSettings`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ParticleOrdering {
    /// All before the deferred passes.
    Before,
    /// Opaque before, translucent after.
    #[default]
    Mixed,
    /// All after translucent terrain.
    After,
}

/// The pack's effect directives and ids, read once at load.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EffectSettings {
    pub clouds: CloudSetting,
    pub sun: bool,
    pub moon: bool,
    pub stars: bool,
    pub weather: bool,
    /// `rain.depth`: weather writes depth.
    pub rain_depth: bool,
    pub particles: ParticleOrdering,
    /// `entityId` of `minecraft:lightning_bolt` from `entity.properties`, else -1.
    pub lightning_entity: i32,
}

impl Default for EffectSettings {
    fn default() -> Self {
        Self {
            clouds: CloudSetting::Default,
            sun: true,
            moon: true,
            stars: true,
            weather: true,
            rain_depth: false,
            particles: ParticleOrdering::Mixed,
            lightning_entity: -1,
        }
    }
}

impl EffectSettings {
    /// Reads the directives as Iris's `ShaderProperties` does. Without an ordering directive
    /// particles draw after the deferred passes when the pack has any (and no
    /// `separateEntityDraws`), else mixed.
    pub(crate) fn read(
        properties: &ShaderProperties,
        has_deferred: bool,
        lightning_entity: Option<i32>,
    ) -> Self {
        let flag = |key: &str, default: bool| match properties.get(key).map(str::trim) {
            Some("true") => true,
            Some("false") => false,
            _ => default,
        };
        let clouds = match properties.get("clouds").map(str::trim) {
            Some("off") => CloudSetting::Off,
            Some("fast") => CloudSetting::Fast,
            Some("fancy") => CloudSetting::Fancy,
            _ => CloudSetting::Default,
        };
        let weather = properties
            .get("weather")
            .and_then(|v| v.split_whitespace().next())
            .is_none_or(|first| first == "true");
        let separate = properties.get("separateEntityDraws").is_some();
        let ordering = properties
            .entries
            .iter()
            .rev()
            .find(|(key, _)| key.starts_with("particles.ordering"))
            .and_then(
                |(_, value)| match value.trim().to_ascii_uppercase().as_str() {
                    "BEFORE" => Some(ParticleOrdering::Before),
                    "MIXED" => Some(ParticleOrdering::Mixed),
                    "AFTER" => Some(ParticleOrdering::After),
                    _ => None,
                },
            );
        let particles = ordering
            .or_else(|| separate.then_some(ParticleOrdering::Mixed))
            .or_else(|| {
                flag("particles.before.deferred", false).then_some(ParticleOrdering::Before)
            })
            .unwrap_or(if has_deferred && !separate {
                ParticleOrdering::After
            } else {
                ParticleOrdering::Mixed
            });
        Self {
            clouds,
            sun: flag("sun", true),
            moon: flag("moon", true),
            stars: flag("stars", true),
            weather,
            rain_depth: flag("rain.depth", false),
            particles,
            lightning_entity: lightning_entity.unwrap_or(-1),
        }
    }

    /// Whether the pack draws `effect` at all.
    pub(crate) fn draws(&self, effect: Effect) -> bool {
        match effect {
            Effect::Sun => self.sun,
            Effect::Moon => self.moon,
            Effect::Stars => self.stars,
            Effect::Clouds => self.clouds != CloudSetting::Off,
            Effect::Weather => self.weather,
            _ => true,
        }
    }

    /// Whether `effect` draws after the deferred passes, with translucent terrain.
    pub(crate) fn after_deferred(&self, effect: Effect) -> bool {
        match effect {
            Effect::Particles => self.particles == ParticleOrdering::After,
            Effect::ParticlesTranslucent | Effect::ParticlesAdditive => {
                self.particles != ParticleOrdering::Before
            }
            Effect::Clouds | Effect::Weather => true,
            _ => false,
        }
    }

    /// `entityId` during `effect`'s draw: the lightning bolt's id, else Iris's -1.
    pub(crate) fn entity_id(&self, effect: Effect) -> i32 {
        if effect == Effect::Lightning {
            self.lightning_entity
        } else {
            -1
        }
    }
}

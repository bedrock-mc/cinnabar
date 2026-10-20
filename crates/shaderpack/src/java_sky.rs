//! Java Edition's sky and fog colours, as Iris hands them to packs: `skyColor` is the
//! level's sky colour at the camera entity and `fogColor` the colour the fog renderer
//! computes for the frame (which Iris also clears `colortex0` to).
//!
//! Behaviour follows Java Edition as the Minecraft Wiki describes it:
//! - Biomes and dimension types set sky, fog and water fog colours through environment
//!   attributes (the pinned Java mapping carrier keeps them); a biome's colour overrides its
//!   dimension type's. The level blends each over the biomes around the camera with a
//!   Gaussian kernel; the caller supplies that blend as weighted biomes
//!   ([`BiomeColors::blend`]).
//! - The sky colour dims with the time of day and greys out in rain and thunder.
//! - The fog colour dims with the time of day, takes the sunrise/sunset glow when the camera
//!   faces the sun's side of the sky, mixes towards the sky colour more the shorter the
//!   render distance, and darkens in rain, in thunder and below the world's floor.
//!
//! Not modelled: lightning flashes (no bolt source), the boss-fight darkening, blindness and
//! darkness fog, and the underwater colour easing between biomes and the water-vision ramp
//! (the biome's water fog colour is used directly).

use assets::facts::java_mapping::{JavaBiomeEntry, JavaBlockMapping, JavaDimension};

/// `minecraft:visual/water_fog_color`'s default, for biomes and dimensions that set none.
const DEFAULT_WATER_FOG: u32 = 0x05_05_33;
/// Fog colours in lava and in powder snow.
const LAVA_FOG: [f32; 3] = [0.6, 0.1, 0.0];
const POWDER_SNOW_FOG: [f32; 3] = [0.623, 0.734, 0.785];

/// An 8-bit `0xRRGGBB` colour as floats.
fn rgb(colour: u32) -> [f32; 3] {
    [16, 8, 0].map(|shift| ((colour >> shift) & 0xFF) as f32 / 255.0)
}

/// Blends two colours by the supplied fraction.
fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// A Java biome's sky, fog and water fog colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BiomeColors {
    pub sky: [f32; 3],
    pub fog: [f32; 3],
    pub water_fog: [f32; 3],
}

impl BiomeColors {
    /// Every colour 0, for blending.
    const ZERO: BiomeColors = BiomeColors {
        sky: [0.0; 3],
        fog: [0.0; 3],
        water_fog: [0.0; 3],
    };

    /// The colours of `entry` in `dimension`: each the biome's own, else the dimension
    /// type's, else the attribute's default (black, and [`DEFAULT_WATER_FOG`]).
    pub fn of(
        entry: &JavaBiomeEntry<'_>,
        mapping: &JavaBlockMapping,
        dimension: JavaDimension,
    ) -> Self {
        let colours = entry
            .colours
            .over(mapping.java_dimension_colours(dimension));
        Self {
            sky: rgb(colours.sky.unwrap_or(0)),
            fog: rgb(colours.fog.unwrap_or(0)),
            water_fog: rgb(colours.water_fog.unwrap_or(DEFAULT_WATER_FOG)),
        }
    }

    /// The colours of the Java biome of Bedrock biome `bedrock_biome` in `dimension`;
    /// unmapped biomes read as the void, as for the biome uniforms. `None` without a Java
    /// biome registry.
    pub fn at(
        mapping: &JavaBlockMapping,
        bedrock_biome: u32,
        dimension: JavaDimension,
    ) -> Option<Self> {
        let index = mapping
            .java_biome_index(bedrock_biome)
            .or_else(|| crate::biomes::fallback_biome_index(mapping))?;
        mapping
            .java_biome_entry(index)
            .map(|entry| Self::of(&entry, mapping, dimension))
    }

    /// The colours of the void in `dimension`: what a camera without streamed biomes sees.
    pub fn fallback(mapping: &JavaBlockMapping, dimension: JavaDimension) -> Option<Self> {
        let index = crate::biomes::fallback_biome_index(mapping)?;
        mapping
            .java_biome_entry(index)
            .map(|entry| Self::of(&entry, mapping, dimension))
    }

    /// The weighted average of `samples`; `None` when no sample has weight.
    pub fn blend(samples: impl IntoIterator<Item = (BiomeColors, f32)>) -> Option<Self> {
        let (sum, total) = samples.into_iter().filter(|(_, w)| *w > 0.0).fold(
            (Self::ZERO, 0.0f32),
            |(sum, total), (colours, w)| {
                let add = |a: [f32; 3], b: [f32; 3]| std::array::from_fn(|i| a[i] + b[i] * w);
                (
                    Self {
                        sky: add(sum.sky, colours.sky),
                        fog: add(sum.fog, colours.fog),
                        water_fog: add(sum.water_fog, colours.water_fog),
                    },
                    total + w,
                )
            },
        );
        (total > 0.0).then(|| {
            let scale = |c: [f32; 3]| c.map(|v| v / total);
            Self {
                sky: scale(sum.sky),
                fog: scale(sum.fog),
                water_fog: scale(sum.water_fog),
            }
        })
    }
}

/// Returns the lowest block Y, below which dimension fog fades to black.
fn dimension_floor(dimension: JavaDimension) -> f32 {
    match dimension {
        JavaDimension::Overworld => -64.0,
        JavaDimension::Nether | JavaDimension::End => 0.0,
    }
}

/// The medium the camera is in, which replaces the fog colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FogMedium {
    Air,
    Water,
    Lava,
    PowderSnow,
}

/// The state the sky and fog colours depend on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyFogInputs {
    /// The biome colours blended around the camera.
    pub biome: BiomeColors,
    pub dimension: JavaDimension,
    /// Fraction of a turn with 0 at noon (Java's time of day).
    pub celestial_angle: f32,
    pub rain: f32,
    pub thunder: f32,
    /// Render distance in chunks.
    pub render_distance_chunks: f32,
    /// The camera's world-space view direction, unit length.
    pub look: [f32; 3],
    /// The camera's world Y.
    pub camera_y: f32,
    /// Positive void-darkness onset range: 32 for normal worlds, 1 for flat worlds.
    pub void_darkness_range: f32,
    pub medium: FogMedium,
    /// Night vision strength `0..1`.
    pub night_vision: f32,
}

/// `skyColor` and `fogColor`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyFogColors {
    pub sky: [f32; 3],
    pub fog: [f32; 3],
}

/// How bright the sky is at `celestial_angle`: full from mid-morning to mid-afternoon, dark
/// through the night.
fn daylight(celestial_angle: f32) -> f32 {
    ((celestial_angle * std::f32::consts::TAU).cos() * 2.0 + 0.5).clamp(0.0, 1.0)
}

/// The sunrise/sunset glow at `celestial_angle`, RGB plus its strength; `None` while the sun
/// is well above or below the horizon.
pub fn sunrise_color(celestial_angle: f32) -> Option<[f32; 4]> {
    let height = (celestial_angle * std::f32::consts::TAU).cos();
    if !(-0.4..=0.4).contains(&height) {
        return None;
    }
    let g = height / 0.4 * 0.5 + 0.5;
    let strength = 1.0 - (1.0 - (g * std::f32::consts::PI).sin()) * 0.99;
    Some([g * 0.3 + 0.7, g * g * 0.7 + 0.2, 0.2, strength * strength])
}

/// The level's sky colour: the biome sky dimmed by the time of day, then greyed by rain and
/// by thunder. Only the overworld has a time of day; the nether's is fixed at midnight and
/// the end's biomes have a black sky, so both are black.
fn sky_color(inputs: &SkyFogInputs) -> [f32; 3] {
    if inputs.dimension != JavaDimension::Overworld {
        return [0.0; 3];
    }
    let mut colour = inputs
        .biome
        .sky
        .map(|c| c * daylight(inputs.celestial_angle));
    let luminance = |c: [f32; 3]| c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11;
    for (level, brightness, factor) in [
        (inputs.rain - inputs.thunder, 0.6, 0.75),
        (inputs.thunder, 0.24, 0.94),
    ] {
        if level > 0.0 {
            let grey = luminance(colour) * brightness;
            colour = mix(colour, [grey; 3], level * factor);
        }
    }
    colour
}

/// Applies the rain-only and thunder RGB attribute layers in order.
fn weather_color(mut colour: [f32; 3], inputs: &SkyFogInputs) -> [f32; 3] {
    for (level, multiplier) in [
        (inputs.rain - inputs.thunder, rgb(0x7F_7F_99)),
        (inputs.thunder, rgb(0x3F_3F_4C)),
    ] {
        if level > 0.0 {
            let target = std::array::from_fn(|i| colour[i] * multiplier[i]);
            colour = mix(colour, target, level);
        }
    }
    colour
}

/// Darkens the sky contribution to atmospheric fog for the current weather.
fn fog_sky_color([r, g, b]: [f32; 3], inputs: &SkyFogInputs) -> [f32; 3] {
    let rain = inputs.rain;
    [
        r * (1.0 - rain * 0.5),
        g * (1.0 - rain * 0.5),
        b * (1.0 - rain * 0.4),
    ]
    .map(|channel| channel * (1.0 - inputs.thunder * 0.5))
}

/// The fog colour in air: the biome fog dimmed by the time of day in the overworld, the
/// sunrise glow towards the sun's side, then mixed towards the sky by the render distance.
/// The nether's and the end's fog colours are used as they are (the end's attribute already
/// carries its dimming).
fn air_fog(inputs: &SkyFogInputs, sky: [f32; 3]) -> [f32; 3] {
    let light = daylight(inputs.celestial_angle);
    let mut fog = match inputs.dimension {
        JavaDimension::Overworld => {
            let [r, g, b] = inputs.biome.fog;
            [
                r * (light * 0.94 + 0.06),
                g * (light * 0.94 + 0.06),
                b * (light * 0.91 + 0.09),
            ]
        }
        JavaDimension::Nether | JavaDimension::End => inputs.biome.fog,
    };
    fog = weather_color(fog, inputs);
    let chunks = inputs.render_distance_chunks;
    if inputs.dimension == JavaDimension::Overworld && chunks >= 4.0 {
        let sun_side = if (inputs.celestial_angle * std::f32::consts::TAU).sin() > 0.0 {
            -1.0
        } else {
            1.0
        };
        let facing = (inputs.look[0] * sun_side).max(0.0);
        if let Some([r, g, b, strength]) = sunrise_color(inputs.celestial_angle)
            && facing > 0.0
        {
            fog = mix(fog, weather_color([r, g, b], inputs), facing * strength);
        }
    }
    let distance = 1.0 - (0.25 + 0.75 * chunks.min(32.0) / 32.0).powf(0.25);
    mix(fog, fog_sky_color(sky, inputs), distance)
}

/// `skyColor` and `fogColor` for one frame and view.
pub fn sky_fog(inputs: &SkyFogInputs) -> SkyFogColors {
    let sky = sky_color(inputs);
    let mut fog = match inputs.medium {
        FogMedium::Air => air_fog(inputs, sky),
        FogMedium::Water => inputs.biome.water_fog,
        FogMedium::Lava => LAVA_FOG,
        FogMedium::PowderSnow => POWDER_SNOW_FOG,
    };
    let above_floor =
        (inputs.camera_y - dimension_floor(inputs.dimension)) / inputs.void_darkness_range;
    if above_floor < 1.0 && matches!(inputs.medium, FogMedium::Air | FogMedium::Water) {
        let fade = above_floor.max(0.0);
        fog = fog.map(|c| c * fade * fade);
    }
    let vision = inputs.night_vision;
    if vision > 0.0 && inputs.medium != FogMedium::Water {
        let brightest = fog.into_iter().fold(0.0f32, f32::max);
        if fog.into_iter().all(|channel| channel != 0.0) {
            fog = fog.map(|c| c * (1.0 - vision) + c / brightest * vision);
        }
    }
    SkyFogColors { sky, fog }
}

#[cfg(test)]
mod tests;

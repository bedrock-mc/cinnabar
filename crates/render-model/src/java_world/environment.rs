//! Evaluates the optional world's weather, visibility and light attributes.

use super::JavaLightmapInputs;
use assets::java_world::{Attribute as A, AttributeValue, JavaEnvironment};

/// Camera and player inputs supplied by the owning client systems.
#[derive(Clone, Copy, Debug)]
pub struct JavaCameraInputs {
    pub render_distance: f32,
    pub forward: [f32; 3],
    pub rain: f32,
    pub thunder: f32,
    pub water: bool,
    pub lava: bool,
    pub spectator: bool,
    pub fire_resistance: bool,
    pub brightness: f32,
    pub night_vision: f32,
    pub blindness: f32,
    pub fog_darkness: f32,
    pub darkness: f32,
    pub darkness_pulse: f32,
    pub sky_exposure: f32,
    pub precipitation: bool,
}

/// Time-dependent visual state; the caller owns clocks and session identity.
#[derive(Clone, Debug, Default)]
pub struct JavaVisualState {
    generation: Option<u64>,
    ticks: f64,
    water_ticks: f32,
    rain_fog: f32,
}

/// Evaluated values shared by all world draw paths in one frame.
#[derive(Clone, Copy, Debug)]
pub struct JavaWorldFrame {
    pub environment: JavaEnvironment,
    pub lightmap: JavaLightmapInputs,
    pub fog_colour: [f32; 3],
    pub fog: [f32; 2],
    pub render_fog: [f32; 2],
    pub sky_fog_end: f32,
    pub cloud_fog_end: f32,
    pub water_vision: f32,
}

impl JavaVisualState {
    /// Advances visual adaptation from caller-supplied renderer ticks and samples this frame.
    pub fn evaluate(
        &mut self,
        generation: u64,
        ticks: f64,
        environment: JavaEnvironment,
        camera: JavaCameraInputs,
    ) -> JavaWorldFrame {
        let ticks = if ticks.is_finite() {
            ticks.max(0.0)
        } else {
            self.ticks
        };
        if self.generation != Some(generation) {
            *self = Self {
                generation: Some(generation),
                ticks,
                ..Self::default()
            };
        }
        let elapsed = (ticks - self.ticks).max(0.0);
        let steps = (ticks.floor() - self.ticks.floor()).max(0.0) as f32;
        self.ticks = self.ticks.max(ticks);
        self.water_ticks = (self.water_ticks
            + steps
                * if camera.water {
                    if camera.spectator { 10.0 } else { 1.0 }
                } else {
                    -10.0
                })
        .clamp(0.0, 600.0);
        let target = camera.rain
            * camera.sky_exposure.clamp(0.0, 1.0)
            * if camera.precipitation { 1.0 } else { 0.5 };
        self.rain_fog += (target - self.rain_fog) * (elapsed as f32 * 0.2).clamp(0.0, 1.0);
        let water_vision = if camera.water {
            (self.water_ticks / 100.0).min(1.0) * 0.6
                + ((self.water_ticks - 100.0) / 500.0).clamp(0.0, 1.0) * 0.399_999_98
        } else {
            0.0
        };
        evaluate(environment, camera, self.rain_fog, water_vision)
    }
}

/// Composes authored time/biome inputs, weather layers, camera fog and player light effects.
fn evaluate(
    mut environment: JavaEnvironment,
    camera: JavaCameraInputs,
    rain_fog: f32,
    water_vision: f32,
) -> JavaWorldFrame {
    weather(&mut environment, camera.rain, camera.thunder);
    let distance = camera.render_distance.max(1.0);
    let span = (distance / 10.0).clamp(4.0, 64.0);
    let mut fog = [
        environment.value(A::FogStart).scalar() - 160.0 * rain_fog,
        (environment.value(A::FogEnd).scalar() - 256.0 * rain_fog)
            .max(environment.value(A::FogEnd).scalar().min(96.0)),
    ];
    let mut sky_end = environment.value(A::SkyFogEnd).scalar().min(distance);
    let mut cloud_end = environment.value(A::CloudFogEnd).scalar();
    let mut colour = environment.value(A::FogColour);
    if camera.water {
        colour = environment.value(A::WaterFogColour);
        fog = [
            environment.value(A::WaterFogStart).scalar(),
            environment.value(A::WaterFogEnd).scalar() * water_vision.max(0.25),
        ];
        sky_end = fog[1];
        cloud_end = fog[1];
    } else if camera.lava {
        colour = AttributeValue([153.0 / 255.0, 25.0 / 255.0, 0.0, 1.0]);
        fog = if camera.spectator {
            [-8.0, distance * 0.5]
        } else if camera.fire_resistance {
            [0.0, 5.0]
        } else {
            [0.25, 1.0]
        };
        sky_end = fog[1];
        cloud_end = fog[1];
    } else {
        let sun_side = if environment.value(A::SunAngle).scalar().to_radians().sin() > 0.0 {
            -1.0
        } else {
            1.0
        };
        if distance >= 64.0 {
            let weight =
                (camera.forward[0] * sun_side).max(0.0) * environment.value(A::Sunrise).0[3];
            colour = colour.mix(environment.value(A::Sunrise), weight);
        }
        let sky = environment.value(A::SkyColour).rgb();
        let sky: [f32; 3] = std::array::from_fn(|channel| {
            sky[channel]
                * (1.0 - camera.rain * if channel == 2 { 0.4 } else { 0.5 })
                * (1.0 - camera.thunder * 0.5)
        });
        let factor = 1.0 - (0.25 + 0.75 * (sky_end / 512.0).clamp(0.0, 1.0)).powf(0.25);
        colour = colour.mix(AttributeValue([sky[0], sky[1], sky[2], 1.0]), factor);
    }
    if !camera.lava {
        let blindness = camera.blindness.clamp(0.0, 1.0);
        let darkness = camera.fog_darkness.clamp(0.0, 1.0);
        if blindness > 0.0 {
            let end = distance + (5.0 - distance) * blindness;
            fog = [end * 0.25, end];
            sky_end = end * 0.8;
            cloud_end = sky_end;
        } else if darkness > 0.0 {
            let end = distance + (15.0 - distance) * darkness;
            fog = [end * 0.75, end];
            sky_end = end;
            cloud_end = end;
        }
        let brightness = (1.0 - blindness.max(darkness)).powi(2);
        for channel in &mut colour.0[..3] {
            *channel *= brightness;
        }
    }
    let brightening = if camera.water {
        water_vision
    } else {
        if camera.fog_darkness > 0.0 {
            0.0
        } else {
            camera.night_vision
        }
    };
    let maximum = colour.rgb().into_iter().fold(0.0_f32, f32::max);
    if colour.rgb().into_iter().all(|value| value != 0.0) && maximum > 0.0 {
        for channel in &mut colour.0[..3] {
            *channel *= 1.0 + (maximum.recip() - 1.0) * brightening;
        }
    }
    let lightmap = JavaLightmapInputs {
        ambient: environment.value(A::Ambient).rgb(),
        sky_colour: environment.value(A::SkyLightColour).rgb(),
        sky_factor: environment.value(A::SkyLightFactor).scalar(),
        block_tint: environment.value(A::BlockTint).rgb(),
        block_factor: 1.4,
        night_vision_colour: environment.value(A::NightVisionColour).rgb(),
        night_vision: camera.night_vision,
        brightness: (camera.brightness - camera.darkness).max(0.0),
        darkness: camera.darkness_pulse,
        boss_darken: 0.0,
    };
    JavaWorldFrame {
        environment,
        lightmap,
        fog_colour: colour.rgb(),
        fog,
        render_fog: [distance - span, distance],
        sky_fog_end: sky_end,
        cloud_fog_end: cloud_end,
        water_vision,
    }
}

/// Applies rain and thunder as separate attribute layers, retaining the source's byte colours.
fn weather(environment: &mut JavaEnvironment, rain: f32, thunder: f32) {
    let thunder = thunder.clamp(0.0, 1.0) * rain.clamp(0.0, 1.0);
    let rain = (rain - thunder).clamp(0.0, 1.0);
    for (
        weight,
        sky_gray,
        cloud_gray,
        desaturate,
        cloud_mix,
        fog_scale,
        light_alpha,
        light_weight,
    ) in [
        (
            rain,
            0.6,
            0.24,
            0.75,
            0.5,
            [127.0 / 255.0, 127.0 / 255.0, 153.0 / 255.0],
            79.0 / 255.0,
            0.3125,
        ),
        (
            thunder,
            0.24,
            0.095,
            0.94,
            0.94,
            [63.0 / 255.0, 63.0 / 255.0, 76.0 / 255.0],
            134.0 / 255.0,
            0.527_343_75,
        ),
    ] {
        if weight <= 0.0 {
            continue;
        }
        for (attribute, scale, amount) in [
            (A::SkyColour, sky_gray, desaturate),
            (A::CloudColour, cloud_gray, cloud_mix),
        ] {
            let base = environment.value(attribute);
            let grey = (base.0[0] * 0.3 + base.0[1] * 0.59 + base.0[2] * 0.11) * scale;
            environment.set(
                attribute,
                base.mix(
                    AttributeValue([grey, grey, grey, base.0[3]]),
                    amount * weight,
                ),
            );
        }
        for attribute in [A::FogColour, A::Sunrise] {
            let base = environment.value(attribute);
            let target = AttributeValue([
                base.0[0] * fog_scale[0],
                base.0[1] * fog_scale[1],
                base.0[2] * fog_scale[2],
                base.0[3],
            ]);
            environment.set(attribute, base.mix(target, weight));
        }
        let light = environment.value(A::SkyLightColour);
        environment.set(
            A::SkyLightColour,
            light.mix(
                AttributeValue([122.0 / 255.0, 122.0 / 255.0, 1.0, 1.0]),
                weight * light_alpha,
            ),
        );
        let factor = environment.value(A::SkyLightFactor);
        environment.set(
            A::SkyLightFactor,
            factor.mix(AttributeValue([0.24; 4]), weight * light_weight),
        );
        let stars = environment.value(A::Stars);
        environment.set(
            A::Stars,
            AttributeValue(stars.0.map(|channel| channel * (1.0 - weight))),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Supplies an unobstructed camera so each test changes only the effect under examination.
    fn camera() -> JavaCameraInputs {
        JavaCameraInputs {
            render_distance: 256.0,
            forward: [0.0, 0.0, 1.0],
            rain: 0.0,
            thunder: 0.0,
            water: false,
            lava: false,
            spectator: false,
            fire_resistance: false,
            brightness: 0.0,
            night_vision: 0.0,
            blindness: 0.0,
            fog_darkness: 0.0,
            darkness: 0.0,
            darkness_pulse: 0.0,
            sky_exposure: 1.0,
            precipitation: true,
        }
    }

    #[test]
    fn water_adaptation_uses_ticks_and_resets_with_the_session() {
        let mut state = JavaVisualState::default();
        let camera = JavaCameraInputs {
            water: true,
            ..camera()
        };
        let environment = JavaEnvironment::default();
        let first = state.evaluate(1, 0.0, environment, camera);
        let adapted = state.evaluate(1, 600.0, environment, camera);
        assert_eq!(first.water_vision, 0.0);
        assert!((adapted.water_vision - 1.0).abs() < f32::EPSILON);
        assert_eq!(
            state.evaluate(2, 600.0, environment, camera).water_vision,
            0.0
        );
    }

    #[test]
    fn blindness_closes_visibility_but_lava_keeps_its_medium_range() {
        let camera = JavaCameraInputs {
            blindness: 1.0,
            water: true,
            ..camera()
        };
        let blind = evaluate(JavaEnvironment::default(), camera, 0.0, 0.0);
        assert_eq!(blind.fog, [1.25, 5.0]);
        assert_eq!(blind.fog_colour, [0.0; 3]);
        let lava = evaluate(
            JavaEnvironment::default(),
            JavaCameraInputs {
                lava: true,
                water: false,
                fire_resistance: true,
                ..camera
            },
            0.0,
            0.0,
        );
        assert_eq!(lava.fog, [0.0, 5.0]);
        assert!(lava.fog_colour[0] > 0.0);
    }
}

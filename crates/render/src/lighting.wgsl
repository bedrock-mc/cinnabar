#define_import_path cinnabar::lighting

const LIGHT_CURVE: array<f32, 16> = array(
    0.0, 0.01754386, 0.037037037, 0.05882353,
    0.083333336, 0.11111111, 0.14285715, 0.17948718,
    0.22222222, 0.27272728, 0.33333334, 0.4074074,
    0.5, 0.61904764, 0.7777778, 1.0,
);

// Provisional conservative floor calibrated to the existing 0.2 horizon
// daylight baseline. Native Bedrock capture tuning remains an acceptance item.
const PROVISIONAL_NIGHT_SKY_TRANSFER_FLOOR: f32 = 0.2;

// Vanilla retains low ambient visibility even when both light channels are
// zero. This conservative linear-light floor remains native-tuning work rather
// than being folded into either independently solved channel.
const PROVISIONAL_ZERO_LIGHT_AMBIENT_FLOOR: f32 = 0.04;

fn lit_colour(
    colour: vec3<f32>,
    block_brightness: f32,
    sky_brightness: f32,
    ao_factor: f32,
    daylight: f32,
) -> vec3<f32> {
    let block_contribution = vec3(clamp(block_brightness, 0.0, 1.0));
    let effective_daylight = max(clamp(daylight, 0.0, 1.0), PROVISIONAL_NIGHT_SKY_TRANSFER_FLOOR);
    let sky_contribution = vec3(
        clamp(sky_brightness, 0.0, 1.0) * effective_daylight,
    );
    let channel_light = max(block_contribution, sky_contribution);
    let combined = mix(
        vec3(PROVISIONAL_ZERO_LIGHT_AMBIENT_FLOOR),
        vec3(1.0),
        channel_light,
    );
    return colour * combined * clamp(ao_factor, 0.0, 1.0);
}

fn light_brightness(level: u32) -> f32 {
    return LIGHT_CURVE[min(level, 15u)];
}

fn light_ao_factor(level: u32) -> f32 {
    return 1.0 - f32(min(level, 3u)) * 0.12;
}

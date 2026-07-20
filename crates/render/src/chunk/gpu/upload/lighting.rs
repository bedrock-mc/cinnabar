use crate::chunk::*;

#[cfg(test)]
pub(in crate::chunk) const PROVISIONAL_NIGHT_SKY_TRANSFER_FLOOR: f32 = 0.2;
#[cfg(test)]
pub(in crate::chunk) const PROVISIONAL_ZERO_LIGHT_AMBIENT_FLOOR: f32 = 0.04;

#[cfg(test)]
pub(in crate::chunk) fn packed_light_factor(sample: u16, daylight: f32) -> f32 {
    const CURVE: [f32; 16] = [
        0.0,
        0.017_543_86,
        0.037_037_037,
        0.058_823_53,
        0.083_333_336,
        0.111_111_11,
        0.142_857_15,
        0.179_487_18,
        0.222_222_22,
        0.272_727_28,
        0.333_333_34,
        0.407_407_4,
        0.5,
        0.619_047_64,
        0.777_777_8,
        1.0,
    ];
    let block_light = CURVE[usize::from(sample & 0x0f)];
    let effective_daylight = daylight
        .clamp(0.0, 1.0)
        .max(PROVISIONAL_NIGHT_SKY_TRANSFER_FLOOR);
    let sky_light = CURVE[usize::from((sample >> 4) & 0x0f)] * effective_daylight;
    let ao = f32::from((sample >> 8) & 0x03);
    let channel_light = block_light.max(sky_light);
    (PROVISIONAL_ZERO_LIGHT_AMBIENT_FLOOR
        + (1.0 - PROVISIONAL_ZERO_LIGHT_AMBIENT_FLOOR) * channel_light)
        * (1.0 - ao * 0.12)
}

pub(in crate::chunk) fn packed_lighting_records(lighting: &[PackedQuadLighting]) -> Vec<[u16; 4]> {
    lighting
        .iter()
        .copied()
        .map(PackedQuadLighting::samples)
        .collect()
}

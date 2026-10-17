use crate::{FogMedium, FogProfile, ResolvedFog};

/// Resolves highest-priority layers first, retaining missing entries' weight for lower layers.
/// Fog layers are supplied in ascending priority.
#[must_use]
pub fn resolve_fog_layers(
    layers: &[&[Option<&FogProfile>]],
    fallback: Option<&FogProfile>,
    medium: FogMedium,
    render_distance: f32,
    transition_seconds: Option<f32>,
) -> Option<ResolvedFog> {
    let values = resolve_values(layers, fallback, |profile| {
        let fog = profile.distance(medium)?.resolve(render_distance)?;
        Some([fog.start, fog.end, fog.rgb[0], fog.rgb[1], fog.rgb[2]])
    })?;
    let target = ResolvedFog {
        start: values[0],
        end: values[1],
        rgb: [values[2], values[3], values[4]],
    };
    let transition = transition_seconds.and_then(|seconds| {
        let initial = resolve_values(layers, fallback, |profile| {
            Some(
                profile
                    .distance(medium)?
                    .transition?
                    .resolve(render_distance),
            )
        })?;
        Some(crate::fog_transition::apply(initial, target, seconds))
    });
    Some(transition.unwrap_or(target))
}

/// Applies identical coverage weights to distance, RGB and transition fields without quantization.
fn resolve_values<const N: usize>(
    layers: &[&[Option<&FogProfile>]],
    fallback: Option<&FogProfile>,
    values: impl Fn(&FogProfile) -> Option<[f32; N]>,
) -> Option<[f32; N]> {
    let mut total = [0.0; N];
    let mut remaining = 1.0;
    let mut found = false;
    for layer in layers.iter().rev().filter(|layer| !layer.is_empty()) {
        let mut present = 0;
        let mut sum = [0.0; N];
        for profile in layer.iter().flatten() {
            if let Some(fields) = values(profile) {
                accumulate(&mut sum, fields, 1.0);
                present += 1;
                found = true;
            }
        }
        accumulate(&mut total, sum, remaining / layer.len() as f32);
        if present == layer.len() {
            remaining = 0.0;
            break;
        }
        remaining *= 1.0 - present as f32 / layer.len() as f32;
    }
    if remaining > 0.0 {
        accumulate(&mut total, values(fallback?)?, remaining);
        found = true;
    }
    found.then_some(total)
}

/// Adds resolved fields using the layer's remaining coverage.
fn accumulate<const N: usize>(total: &mut [f32; N], values: [f32; N], weight: f32) {
    for (sum, value) in total.iter_mut().zip(values) {
        *sum += weight * value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FogDistance, FogDistanceMode};

    /// Builds one optional water setting for the layer arithmetic fixtures.
    fn profile(end: Option<f32>) -> FogProfile {
        FogProfile {
            identifier: "fixture".into(),
            distances: end
                .into_iter()
                .map(|end| FogDistance {
                    medium: FogMedium::Water,
                    mode: FogDistanceMode::Fixed,
                    start_bits: 0.0_f32.to_bits(),
                    end_bits: end.to_bits(),
                    rgb8: 0x804020,
                    transition: None,
                })
                .collect(),
        }
    }

    #[test]
    fn missing_entries_preserve_lower_layer_and_default_weights() {
        let high = profile(Some(20.0));
        let low = profile(Some(40.0));
        let missing = profile(None);
        let fallback = profile(Some(80.0));
        let fog = resolve_fog_layers(
            &[
                &[Some(&low), Some(&missing)],
                &[Some(&high), Some(&missing)],
            ],
            Some(&fallback),
            FogMedium::Water,
            128.0,
            None,
        )
        .unwrap();
        assert_eq!(fog.end, 40.0); // 20/2 + 40/4 + 80/4.
        assert_eq!(fog.rgb, [128.0 / 255.0, 64.0 / 255.0, 32.0 / 255.0]);
    }

    #[test]
    fn transition_fields_blend_before_the_timeline_without_rgb_quantization() {
        let mut first = profile(Some(20.0));
        let mut second = profile(Some(60.0));
        first.distances[0].transition = Some(crate::FogTransition {
            mode: FogDistanceMode::Fixed,
            start_bits: 0.0_f32.to_bits(),
            end_bits: 0.1_f32.to_bits(),
            rgb8: 0,
            min_percent_bits: 0.2_f32.to_bits(),
            mid_seconds_bits: 2.0_f32.to_bits(),
            mid_percent_bits: 0.4_f32.to_bits(),
            max_seconds_bits: 10.0_f32.to_bits(),
        });
        second.distances[0].transition = Some(crate::FogTransition {
            mode: FogDistanceMode::Fixed,
            start_bits: 0.0_f32.to_bits(),
            end_bits: 0.3_f32.to_bits(),
            rgb8: 0xffffff,
            min_percent_bits: 0.4_f32.to_bits(),
            mid_seconds_bits: 6.0_f32.to_bits(),
            mid_percent_bits: 0.8_f32.to_bits(),
            max_seconds_bits: 30.0_f32.to_bits(),
        });
        let layers = [&[Some(&first), Some(&second)][..]];
        let halfway =
            resolve_fog_layers(&layers, None, FogMedium::Water, 128.0, Some(2.0)).unwrap();
        assert!((halfway.end - 12.14).abs() < 0.00001);
        assert!((halfway.rgb[0] - (0.35 + (128.0 / 255.0) * 0.3)).abs() < 0.00001);
        let finished =
            resolve_fog_layers(&layers, None, FogMedium::Water, 128.0, Some(20.0)).unwrap();
        assert_eq!(finished.end, 40.0);
    }

    #[test]
    fn complete_layer_stops_and_empty_layers_have_no_weight() {
        let high = profile(Some(20.0));
        let low = profile(Some(90.0));
        assert_eq!(
            resolve_fog_layers(
                &[&[Some(&low)], &[], &[Some(&high)]],
                None,
                FogMedium::Water,
                64.0,
                None,
            )
            .unwrap()
            .end,
            20.0
        );
    }
}

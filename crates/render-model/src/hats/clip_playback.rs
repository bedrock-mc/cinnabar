//! Validated cosmetic transform clips and an optional intro followed by a repeating sequence.
use glam::Vec3;
use serde::Deserialize;
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Definition {
    pub r#loop: bool,
    pub animation_length: f32,
    pub bones: BTreeMap<String, Channels>,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Channels {
    #[serde(default)]
    rotation: BTreeMap<String, [f32; 3]>,
    #[serde(default)]
    position: BTreeMap<String, [f32; 3]>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Sequence {
    pub intro: Option<String>,
    pub cycle: Vec<Step>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Step {
    animation: String,
    delay: f32,
}
type Keys = Vec<(f32, Vec3)>;
#[derive(Default)]
struct Track {
    rotation: Keys,
    position: Keys,
}
struct Clip {
    length: f32,
    tracks: Vec<Track>,
}
pub(super) struct Clips {
    clips: Vec<Clip>,
    intro: Option<usize>,
    cycle: Vec<(usize, f32)>,
    intro_length: f32,
    cycle_length: f32,
}

/// Validates bounded keys in authored degrees or model pixels, including numeric time aliases.
fn keys(raw: BTreeMap<String, [f32; 3]>, length: f32, limit: f32, looping: bool) -> Option<Keys> {
    if raw.is_empty() {
        return Some(Vec::new());
    }
    if !(2..=256).contains(&raw.len()) {
        return None;
    }
    let mut keys = Vec::with_capacity(raw.len());
    for (time, value) in raw {
        let time: f32 = time.parse().ok()?;
        if !time.is_finite()
            || !(0.0..=length).contains(&time)
            || value.iter().any(|v| !v.is_finite() || v.abs() > limit)
        {
            return None;
        }
        keys.push((time, Vec3::from_array(value)));
    }
    keys.sort_by(|a, b| a.0.total_cmp(&b.0));
    (keys.first()?.0 == 0.0
        && keys.last()?.0 == length
        && (!looping || keys.first()?.1 == keys.last()?.1)
        && !keys.windows(2).any(|k| k[0].0 >= k[1].0))
    .then_some(keys)
}

/// Samples linear authored channels, clamping one-shot clips at either endpoint.
fn sample(keys: &Keys, time: f32) -> Vec3 {
    if keys.is_empty() {
        return Vec3::ZERO;
    }
    let upper = keys
        .partition_point(|key| key.0 <= time)
        .min(keys.len() - 1);
    let (a, b) = (keys[upper.saturating_sub(1)], keys[upper]);
    if a.0 == b.0 {
        a.1
    } else {
        a.1.lerp(b.1, ((time - a.0) / (b.0 - a.0)).clamp(0.0, 1.0))
    }
}
/// Samples authored degrees along the shortest turn between keys, so a key pair that wraps
/// from +180 to -180 continues a spin instead of sweeping back through zero.
fn sample_rotation(keys: &Keys, time: f32) -> Vec3 {
    if keys.is_empty() {
        return Vec3::ZERO;
    }
    let upper = keys
        .partition_point(|key| key.0 <= time)
        .min(keys.len() - 1);
    let (a, b) = (keys[upper.saturating_sub(1)], keys[upper]);
    if a.0 == b.0 {
        return a.1;
    }
    let turn = rotation_delta(a.1, b.1);
    a.1 + turn * ((time - a.0) / (b.0 - a.0)).clamp(0.0, 1.0)
}

/// Returns each axis's shortest signed turn in degrees, treating full turns as equivalent.
fn rotation_delta(from: Vec3, to: Vec3) -> Vec3 {
    (to - from).map(|degrees| (degrees + 180.0).rem_euclid(360.0) - 180.0)
}

impl Clip {
    /// Rejects empty, unknown or oversized bone channels before constructing indexed tracks.
    fn build(definition: Definition, names: &[&str]) -> Option<Self> {
        let length = definition.animation_length;
        if !length.is_finite()
            || !(0.1..=60.0).contains(&length)
            || definition.bones.is_empty()
            || definition.bones.len() > 32
        {
            return None;
        }
        let mut tracks: Vec<Track> = (0..names.len()).map(|_| Track::default()).collect();
        for (name, channels) in definition.bones {
            let bone = names.iter().position(|n| *n == name)?;
            if channels.rotation.is_empty() && channels.position.is_empty() {
                return None;
            }
            tracks[bone] = Track {
                rotation: keys(channels.rotation, length, 180.0, definition.r#loop)?,
                position: keys(channels.position, length, 64.0, definition.r#loop)?,
            };
        }
        Some(Self { length, tracks })
    }

    /// Requires compatible clip endpoints so delays and wrap boundaries cannot teleport bones.
    fn joins(&self, next: &Self) -> bool {
        self.tracks.iter().zip(&next.tracks).all(|(a, b)| {
            sample(&a.position, self.length).distance(sample(&b.position, 0.0)) < 1e-5
                && rotation_delta(
                    sample_rotation(&a.rotation, self.length),
                    sample_rotation(&b.rotation, 0.0),
                )
                .length()
                    < 1e-5
        })
    }
}
impl Clips {
    /// Resolves names once and validates every transition, including the intro and cycle seam.
    pub fn build(
        definitions: BTreeMap<String, Definition>,
        sequence: Option<Sequence>,
        names: &[&str],
    ) -> Option<Self> {
        if definitions.is_empty() || definitions.len() > 8 {
            return None;
        }
        if sequence.is_none()
            && (definitions.len() != 1 || !definitions.first_key_value()?.1.r#loop)
        {
            return None;
        }
        let animation_names: Vec<_> = definitions.keys().cloned().collect();
        let mut clips = Vec::new();
        for definition in definitions.into_values() {
            clips.push(Clip::build(definition, names)?);
        }
        let (intro, cycle) = if let Some(sequence) = sequence {
            if sequence.cycle.is_empty() || sequence.cycle.len() > 16 {
                return None;
            }
            let intro = match sequence.intro {
                Some(name) => Some(animation_names.iter().position(|n| *n == name)?),
                None => None,
            };
            let mut cycle = Vec::new();
            for step in sequence.cycle {
                if !step.delay.is_finite() || !(0.0..=60.0).contains(&step.delay) {
                    return None;
                }
                cycle.push((
                    animation_names.iter().position(|n| *n == step.animation)?,
                    step.delay,
                ));
            }
            (intro, cycle)
        } else {
            (None, vec![(0, 0.0)])
        };
        if (0..clips.len()).any(|i| intro != Some(i) && !cycle.iter().any(|(index, _)| *index == i))
        {
            return None;
        }
        for i in 0..cycle.len() {
            if !clips[cycle[i].0].joins(&clips[cycle[(i + 1) % cycle.len()].0]) {
                return None;
            }
        }
        if intro.is_some_and(|i| !clips[i].joins(&clips[cycle[0].0])) {
            return None;
        }
        let intro_length = intro.map_or(0.0, |i| clips[i].length);
        let cycle_length: f32 = cycle
            .iter()
            .map(|(i, delay)| clips[*i].length + delay)
            .sum();
        if cycle_length > 600.0 {
            return None;
        }
        Some(Self {
            clips,
            intro,
            cycle,
            intro_length,
            cycle_length,
        })
    }

    /// Returns the bounded duration used by the optional continuous-flex loop.
    pub fn length(&self) -> f32 {
        self.intro_length + self.cycle_length
    }

    /// Keeps the intro outside the repeating interval, including after a very large frame delta.
    pub fn normalize(&self, seconds: f64) -> f32 {
        if !seconds.is_finite() || seconds < 0.0 {
            return 0.0;
        }
        let intro = f64::from(self.intro_length);
        let cycle = f64::from(self.cycle_length);
        let time = if seconds < intro {
            seconds
        } else {
            intro + (seconds - intro).rem_euclid(cycle)
        };
        // Rounding immediately below the cycle end must not manufacture a later intro replay.
        let result = time as f32;
        if result >= self.length() {
            self.intro_length
        } else {
            result
        }
    }

    /// Returns the initial perched pose for static thumbnails rather than the fly-in pose.
    pub fn rest_time(&self) -> f32 {
        self.intro_length
    }

    /// Selects one clip without allocations, holding its start pose throughout its delay.
    fn active(&self, seconds: f32) -> (&Clip, f32) {
        let time = self.normalize(f64::from(seconds));
        if time < self.intro_length {
            return (&self.clips[self.intro.unwrap()], time);
        }
        let mut time = time - self.intro_length;
        for &(index, delay) in &self.cycle {
            let clip = &self.clips[index];
            if time < delay + clip.length {
                return (clip, (time - delay).max(0.0));
            }
            time -= delay + clip.length;
        }
        (&self.clips[self.cycle[0].0], 0.0)
    }

    /// Returns one bone's authored degrees and pixel translation at the shared motion time.
    pub fn sample(&self, bone: usize, seconds: f32) -> (Vec3, Vec3) {
        if bone >= self.clips[0].tracks.len() {
            return (Vec3::ZERO, Vec3::ZERO);
        }
        let (clip, time) = self.active(seconds);
        clip.tracks
            .get(bone)
            .map_or((Vec3::ZERO, Vec3::ZERO), |track| {
                (
                    sample_rotation(&track.rotation, time),
                    sample(&track.position, time),
                )
            })
    }

    /// Reports authored tracks for validating continuous-flex parent joints.
    pub fn has_track(&self, bone: usize) -> bool {
        self.clips.iter().any(|clip| {
            clip.tracks
                .get(bone)
                .is_some_and(|track| !track.rotation.is_empty() || !track.position.is_empty())
        })
    }
}

//! Prepared path geometry and independent progress and rotation tracks.

use protocol::{CameraSpline, CameraSplineKind};

use bevy::prelude::{EulerRot, Quat, Vec3};

use super::super::easing::{ease, kind_from_name};
use super::runtime::Pose;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Keyframe {
    value: Vec3,
    time: f32,
    ease: u8,
}

/// Path preparation owns allocations; sampling only reads contiguous prepared arrays.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SplinePlayback {
    points: Box<[Vec3]>,
    knots: Box<[f32]>,
    tangents: Option<Box<[Vec3]>>,
    progress: Box<[Keyframe]>,
    rotation: Box<[Keyframe]>,
    duration: f32,
    elapsed: f32,
    finished: bool,
}

impl SplinePlayback {
    /// Rejects degenerate paths and unordered tracks before preparing their immutable geometry.
    pub(super) fn new(spline: &CameraSpline) -> Option<Self> {
        let minimum = match spline.kind {
            CameraSplineKind::Linear => 3,
            CameraSplineKind::CatmullRom => 4,
        };
        if spline.control_points.len() < minimum
            || !spline.total_time_seconds.is_finite()
            || spline.total_time_seconds <= 0.0
        {
            return None;
        }
        let points: Box<[Vec3]> = spline
            .control_points
            .iter()
            .copied()
            .map(Vec3::from_array)
            .collect();
        if points.iter().any(|point| !point.is_finite()) {
            return None;
        }
        let mut distance = 0.0;
        let mut knots = Vec::with_capacity(points.len() - 1);
        for segment in points.windows(2) {
            knots.push(distance);
            distance += segment[0].distance(segment[1]);
        }
        if !distance.is_finite() || distance <= f32::EPSILON {
            return None;
        }
        for knot in &mut knots {
            *knot /= distance;
        }
        let tangents = (spline.kind == CameraSplineKind::CatmullRom).then(|| {
            (0..points.len())
                .map(|index| {
                    let previous = index.saturating_sub(1);
                    let next = (index + 1).min(points.len() - 1);
                    (points[next] - points[previous]) * 0.5
                })
                .collect()
        });
        let progress: Box<[_]> = spline
            .progress_key_frames
            .iter()
            .map(|frame| Keyframe {
                value: Vec3::new(frame.progress, 0.0, 0.0),
                time: frame.time_seconds,
                ease: kind_from_name(&frame.ease_type),
            })
            .collect();
        let rotation: Box<[_]> = spline
            .rotation_key_frames
            .iter()
            .map(|frame| Keyframe {
                value: Vec3::from_array(frame.rotation_degrees),
                time: frame.time_seconds,
                ease: kind_from_name(&frame.ease_type),
            })
            .collect();
        if !valid_track(&progress, spline.total_time_seconds)
            || !valid_track(&rotation, spline.total_time_seconds)
        {
            return None;
        }
        // Prepare the shared native trigonometric table before the first render-frame sample.
        sim::minecraft_sin(0.0);
        Some(Self {
            points,
            knots: knots.into_boxed_slice(),
            tangents,
            progress,
            rotation,
            duration: spline.total_time_seconds,
            elapsed: 0.0,
            finished: false,
        })
    }

    /// Crossing the duration ends playback without evaluating a later or clamped endpoint.
    pub(super) fn advance(&mut self, delta_seconds: f32) {
        if self.finished {
            return;
        }
        let next = self.elapsed + delta_seconds;
        if next > self.duration {
            self.finished = true;
        } else {
            self.elapsed = next;
        }
    }

    /// Finished playback releases the animated pose back to the stationary camera values.
    pub(super) const fn is_finished(&self) -> bool {
        self.finished
    }

    /// Evaluates progress on the path and interpolates Euler rotations independently.
    pub(super) fn sample(&self) -> Pose {
        let progress = sample_track(&self.progress, self.elapsed, self.duration).x;
        let rotation = sample_track(&self.rotation, self.elapsed, self.duration);
        Pose {
            translation: self.position(progress),
            rotation: Quat::from_euler(
                EulerRot::YXZ,
                rotation.y.to_radians(),
                rotation.x.to_radians(),
                rotation.z.to_radians(),
            ),
        }
    }

    /// The path parameter uses accumulated control-edge distance, including for curved segments.
    fn position(&self, progress: f32) -> Vec3 {
        let index = self
            .knots
            .partition_point(|&knot| knot <= progress)
            .saturating_sub(1);
        let next = index + 1;
        let end = self.knots.get(next).copied().unwrap_or(1.0);
        let width = end - self.knots[index];
        let t = if width > f32::EPSILON {
            (progress - self.knots[index]) / width
        } else {
            0.0
        };
        let start = self.points[index];
        let end = self.points[next];
        let Some(tangents) = &self.tangents else {
            return start.lerp(end, t);
        };
        let t2 = t * t;
        let t3 = t2 * t;
        start * (2.0 * t3 - 3.0 * t2 + 1.0)
            + tangents[index] * (t3 - 2.0 * t2 + t)
            + end * (-2.0 * t3 + 3.0 * t2)
            + tangents[next] * (t3 - t2)
    }
}

/// Empty or unordered tracks have no safe interpolation interval and are skipped.
fn valid_track(track: &[Keyframe], duration: f32) -> bool {
    !track.is_empty()
        && track.iter().all(|frame| {
            frame.time.is_finite()
                && frame.time >= 0.0
                && frame.time <= duration
                && frame.value.is_finite()
        })
        && track.windows(2).all(|pair| pair[0].time <= pair[1].time)
}

/// Each segment uses the easing selector on its starting keyframe; the last value is held.
fn sample_track(track: &[Keyframe], time: f32, duration: f32) -> Vec3 {
    let index = track
        .partition_point(|frame| frame.time <= time)
        .saturating_sub(1);
    let start = track[index];
    let end = track.get(index + 1).copied().unwrap_or(Keyframe {
        time: duration,
        ..start
    });
    let width = end.time - start.time;
    let t = if width > f32::EPSILON {
        (time - start.time) / width
    } else {
        0.0
    };
    start.value.lerp(end.value, ease(start.ease, t))
}

#[cfg(test)]
mod tests;

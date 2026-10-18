//! Bounded compass presentation with a host-owned, current-session pose.

use serde::{Deserialize, Serialize};

/// Tick positions are shared by retained geometry and its changing native bindings.
pub const COMPASS_TICKS: usize = 24;
pub const COMPASS_HEIGHT: f32 = 32.;
pub const COMPASS_MIN_WIDTH: f32 = 168.;
pub const COMPASS_MAX_WIDTH: f32 = 448.;
pub const COMPASS_DEFAULT_WIDTH: f32 = 280.;
pub const MAX_COMPASS_COORDINATE: f32 = 30_000_000.;
pub const MAX_COMPASS_MARKERS: usize = 16;
pub const MAX_HUD_COMPASSES: usize = 4;
pub const COMPASS_DIRECTIONS: [&str; 8] = ["S", "SW", "W", "NW", "N", "NE", "E", "SE"];
const _: () = assert!(COMPASS_TICKS > 0 && COMPASS_TICKS.is_multiple_of(COMPASS_DIRECTIONS.len()));

/// A local waypoint selected by the component for this compass's session and dimension.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompassMarker {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

/// The native host supplies this pose; component JSON cannot provide it.
#[derive(Clone, Debug, PartialEq)]
pub struct CompassFrame {
    /// Bedrock view degrees: south is zero, west is ninety.
    pub heading: f32,
    pub position: [f32; 3],
}

/// Declarative compass policy; the native renderer updates its pose without guest polling.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Compass {
    pub session: u64,
    pub dimension: i32,
    pub color: [f32; 4],
    pub marker_color: [f32; 4],
    pub show_marker: bool,
    pub show_value: bool,
    pub waypoints: Vec<CompassMarker>,
    #[serde(skip)]
    pub frame: Option<CompassFrame>,
}

impl Default for Compass {
    /// Keeps an unbound compass empty until native current-session observations arrive.
    fn default() -> Self {
        Self {
            session: 0,
            dimension: 0,
            color: [1.; 4],
            marker_color: [1.; 4],
            show_marker: true,
            show_value: true,
            waypoints: Vec::new(),
            frame: None,
        }
    }
}

impl Compass {
    /// Bounds authored marker data and native observations before either reaches layout.
    pub(super) fn validate(&self) -> Result<(), String> {
        super::color(self.color)?;
        super::color(self.marker_color)?;
        if self.waypoints.len() > MAX_COMPASS_MARKERS {
            return Err("compass exceeds its waypoint limit".into());
        }
        for marker in &self.waypoints {
            super::color(marker.color)?;
            if !position_valid(marker.position) {
                return Err("compass waypoint position exceeds its bounds".into());
            }
        }
        if self
            .frame
            .as_ref()
            .is_some_and(|frame| !frame.heading.is_finite() || !position_valid(frame.position))
        {
            return Err("compass pose is invalid".into());
        }
        Ok(())
    }
}

/// Accepts finite world coordinates without restricting custom dimension identities.
fn position_valid(position: [f32; 3]) -> bool {
    position
        .into_iter()
        .all(|value| value.is_finite() && value.abs() <= MAX_COMPASS_COORDINATE)
}

/// Projects a bearing into a half-circle viewport, wrapping continuously across north/south.
pub fn compass_offset(bearing: f32, heading: f32, width: f32) -> Option<f32> {
    if !bearing.is_finite() || !heading.is_finite() || !width.is_finite() || width <= 0. {
        return None;
    }
    let delta = (bearing - heading + 180.).rem_euclid(360.) - 180.;
    (delta.abs() <= 90.).then_some(width * 0.5 + delta * (width / 180.))
}

/// Returns the horizontal Bedrock bearing, leaving coincident or unknown positions absent.
pub fn compass_bearing(origin: [f32; 3], target: [f32; 3]) -> Option<f32> {
    if !position_valid(origin) || !position_valid(target) {
        return None;
    }
    let dx = target[0] - origin[0];
    let dz = target[2] - origin[2];
    (dx != 0. || dz != 0.).then(|| (-dx).atan2(dz).to_degrees().rem_euclid(360.))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bearings use the observed Bedrock axes and never invent a direction at one position.
    #[test]
    fn cardinal_bearings_follow_bedrock_axes() {
        for (position, expected) in [
            ([0., 0., 1.], 0.),
            ([-1., 0., 0.], 90.),
            ([0., 0., -1.], 180.),
            ([1., 0., 0.], 270.),
        ] {
            assert_eq!(compass_bearing([0.; 3], position), Some(expected));
        }
        assert_eq!(compass_bearing([1., 0., 1.], [1., 10., 1.]), None);
        assert_eq!(compass_bearing([f32::NAN; 3], [0.; 3]), None);
    }

    /// Wrapping preserves nearby bearings while hiding points behind the current heading.
    #[test]
    fn projection_wraps_and_rejects_non_finite_values() {
        assert_eq!(compass_offset(0., 360., 180.), Some(90.));
        assert_eq!(compass_offset(1., 359., 180.), Some(92.));
        assert_eq!(compass_offset(359., 1., 180.), Some(88.));
        assert_eq!(compass_offset(180., 0., 180.), None);
        assert_eq!(compass_offset(90., 0., 180.), Some(180.));
        assert_eq!(compass_offset(0., f32::INFINITY, 180.), None);
    }

    /// Guest JSON cannot impersonate a native pose or exceed finite marker bounds.
    #[test]
    fn compass_input_is_bounded_and_pose_is_native_only() {
        assert!(
            serde_json::from_str::<Compass>(r#"{"frame":{"heading":0,"position":[0,0,0]}}"#)
                .is_err()
        );
        let mut compass = Compass::default();
        compass.validate().unwrap();
        compass.waypoints = vec![
            CompassMarker {
                position: [0.; 3],
                color: [1.; 4]
            };
            MAX_COMPASS_MARKERS + 1
        ];
        assert!(compass.validate().is_err());
        compass.waypoints.truncate(1);
        compass.waypoints[0].position[0] = f32::NAN;
        assert!(compass.validate().is_err());
    }

    /// Each compass owns its card geometry and shares a finite whole-HUD admission limit.
    #[test]
    fn compass_cards_reject_mixed_content_and_unbounded_replication() {
        use super::super::{Card, Cell, Hud};
        let card = Card {
            id: "direction".into(),
            width: COMPASS_DEFAULT_WIDTH,
            compass: Some(Compass::default()),
            ..Default::default()
        };
        let mut hud = Hud {
            cards: vec![card.clone()],
            ..Default::default()
        };
        hud.validate().unwrap();
        hud.cards[0].cells.push(Cell::default());
        assert!(hud.validate().is_err());
        hud.cards[0] = card.clone();
        hud.cards[0].width = COMPASS_MAX_WIDTH + 1.;
        assert!(hud.validate().is_err());
        hud.cards = (0..=MAX_HUD_COMPASSES)
            .map(|i| Card {
                id: format!("compass_{i}"),
                ..card.clone()
            })
            .collect();
        assert!(hud.validate().is_err());
    }
}

//! Eased presentation of surface bindings that declare transitions, driven by the caller's clock.

use std::collections::HashMap;

use json_ui::{DataSource, Scalar};
use serde_json::json;
use ui::mod_panel::{Surface, SurfaceValue, TransitionEasing};
use ui_engine::engine::glass::Backdrop;

/// One binding moving between two values of the same length.
struct Tween {
    from: Vec<f64>,
    to: Vec<f64>,
    began: f64,
    duration: f64,
    easing: TransitionEasing,
    /// Publishes an array rather than a single number.
    vector: bool,
}

impl Tween {
    /// Eased progress at `now`: exactly 1 once the duration has elapsed.
    fn progress(&self, now: f64) -> f64 {
        let t = if self.duration > 0.0 {
            (now - self.began) / self.duration
        } else {
            1.0
        };
        self.easing.progress(t)
    }

    /// The displayed components at `now`, exactly `to` once the duration has elapsed.
    fn sample(&self, now: f64) -> Vec<f64> {
        let progress = self.progress(now);
        self.from
            .iter()
            .zip(&self.to)
            .map(|(from, to)| from + (to - from) * progress)
            .collect()
    }

    /// The first displayed component at `now`, without allocating.
    fn first(&self, now: f64) -> Option<f64> {
        let (from, to) = (self.from.first()?, self.to.first()?);
        Some(from + (to - from) * self.progress(now))
    }

    /// Whether the displayed value has reached its target by `now`.
    fn settled(&self, now: f64) -> bool {
        self.from == self.to || now >= self.began + self.duration
    }
}

/// Displayed values of transitioned bindings; a fresh instance shows each binding's
/// declared `from`, so surfaces ease in whenever they are opened or replaced.
#[derive(Default)]
pub(crate) struct SurfaceTransitions {
    now: f64,
    tweens: HashMap<String, Tween>,
    /// Bindings that declare an `exit` value ease toward it instead of their current value.
    exiting: bool,
}

impl SurfaceTransitions {
    /// Retargets changed bindings from their displayed values and reports whether any
    /// displayed value differs from the last [`Self::write`]. Without `animate`, every value,
    /// including one already moving, changes at once.
    pub(crate) fn advance(&mut self, surface: &Surface, now: f64, animate: bool) -> bool {
        if !now.is_finite() {
            return false;
        }
        let previous = self.now;
        self.now = now.max(previous);
        let now = self.now;
        self.tweens.retain(|name, _| {
            surface.transitions.contains_key(name)
                && surface
                    .bindings
                    .get(name)
                    .and_then(SurfaceValue::numbers)
                    .is_some()
        });
        let mut changed = false;
        if !animate {
            for tween in self.tweens.values_mut() {
                if !tween.settled(now) {
                    tween.from.clone_from(&tween.to);
                    changed = true;
                }
            }
        }
        for (name, transition) in &surface.transitions {
            let Some(binding) = surface.bindings.get(name) else {
                continue;
            };
            let Some(current) = binding.numbers() else {
                continue;
            };
            let target = transition
                .exit
                .as_ref()
                .and_then(SurfaceValue::numbers)
                .filter(|exit| self.exiting && exit.len() == current.len())
                .unwrap_or(current);
            let retarget = |from: Vec<f64>| Tween {
                from,
                to: target.to_vec(),
                began: now,
                duration: if animate { transition.duration } else { 0.0 },
                easing: transition.easing,
                vector: matches!(binding, SurfaceValue::Vector(_)),
            };
            match self.tweens.get_mut(name) {
                None => {
                    let from = transition
                        .from
                        .as_ref()
                        .and_then(SurfaceValue::numbers)
                        .filter(|from| animate && from.len() == target.len())
                        .unwrap_or(target);
                    self.tweens.insert(name.clone(), retarget(from.to_vec()));
                    changed = true;
                }
                Some(tween) if tween.to != target => {
                    let from = if tween.to.len() == target.len() {
                        tween.sample(now)
                    } else {
                        target.to_vec()
                    };
                    *tween = retarget(from);
                    changed = true;
                }
                Some(tween) => changed |= !tween.settled(previous),
            }
        }
        changed
    }

    /// Starts easing each displayed binding that declares an `exit` value toward it from the
    /// next [`Self::advance`]. Reports whether any value would move; when none would, nothing
    /// changes and the caller can remove the surface at once.
    pub(crate) fn begin_exit(&mut self, surface: &Surface) -> bool {
        let moves = surface.transitions.iter().any(|(name, transition)| {
            let exit = transition.exit.as_ref().and_then(SurfaceValue::numbers);
            match (self.tweens.get(name), exit) {
                (Some(tween), Some(exit)) => {
                    transition.duration > 0.0
                        && exit.len() == tween.to.len()
                        && tween.sample(self.now) != exit
                }
                _ => false,
            }
        });
        self.exiting = moves;
        moves
    }

    /// Returns bindings easing toward their exit values to their current values, continuing
    /// from what is displayed.
    pub(crate) fn cancel_exit(&mut self) {
        self.exiting = false;
    }

    /// Whether bindings are easing toward their exit values.
    pub(crate) fn exiting(&self) -> bool {
        self.exiting
    }

    /// Overwrites each transitioned binding with its displayed value.
    pub(crate) fn write(&self, data: &mut DataSource) {
        for (name, tween) in &self.tweens {
            let value = tween.sample(self.now);
            let scalar = if tween.vector {
                Scalar::Json(json!(value))
            } else {
                Scalar::Num(value[0])
            };
            data.set_global(name.as_str(), scalar);
        }
    }

    /// Whether any displayed value is still moving toward its target.
    pub(crate) fn active(&self) -> bool {
        self.tweens.values().any(|tween| !tween.settled(self.now))
    }

    /// The world backdrop `surface`'s reserved bindings request, each at its displayed value.
    pub(crate) fn world_backdrop(&self, surface: &Surface) -> Backdrop {
        Backdrop::read(|property| {
            match self
                .tweens
                .get(property.world)
                .filter(|tween| !tween.vector)
            {
                Some(tween) => tween.first(self.now),
                None => match surface.bindings.get(property.world) {
                    Some(SurfaceValue::Number(number)) => Some(*number),
                    _ => None,
                },
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use json_ui::Easing;
    use ui::mod_panel::SurfaceTransition;

    use super::*;

    /// A surface with one transitioned binding and no document, enough for the sampler.
    fn surface(value: SurfaceValue, transition: SurfaceTransition) -> Surface {
        Surface {
            collections: Default::default(),
            previews: Default::default(),
            screen: "fixture.root".into(),
            document: String::new(),
            bindings: BTreeMap::from([("#value".into(), value)]),
            transitions: BTreeMap::from([("#value".into(), transition)]),
        }
    }

    /// A linear transition with an optional opening value and no exit value.
    fn linear(duration: f64, from: Option<SurfaceValue>) -> SurfaceTransition {
        SurfaceTransition {
            duration,
            easing: TransitionEasing::Named(Easing::Linear),
            from,
            exit: None,
        }
    }

    /// A linear one-second transition of `#value` from 1 that exits toward `exit`.
    fn exiting(exit: SurfaceValue) -> Surface {
        let mut transition = linear(1.0, None);
        transition.exit = Some(exit);
        surface(SurfaceValue::Number(1.0), transition)
    }

    /// The bindings the transitions publish.
    fn written(transitions: &SurfaceTransitions) -> DataSource {
        let mut data = DataSource::new();
        transitions.write(&mut data);
        data
    }

    /// Data holding only `#value`.
    fn value(scalar: Scalar) -> DataSource {
        let mut data = DataSource::new();
        data.set_global("#value", scalar);
        data
    }

    #[test]
    fn retargeting_mid_transition_continues_from_the_displayed_value() {
        let mut surface = surface(SurfaceValue::Number(0.0), linear(1.0, None));
        let mut transitions = SurfaceTransitions::default();
        assert!(transitions.advance(&surface, 10.0, true));
        assert_eq!(written(&transitions), value(Scalar::Num(0.0)));
        assert!(!transitions.active());

        surface
            .bindings
            .insert("#value".into(), SurfaceValue::Number(1.0));
        assert!(transitions.advance(&surface, 10.0, true));
        assert!(transitions.advance(&surface, 10.5, true));
        assert_eq!(written(&transitions), value(Scalar::Num(0.5)));

        surface
            .bindings
            .insert("#value".into(), SurfaceValue::Number(0.0));
        assert!(transitions.advance(&surface, 10.5, true));
        assert_eq!(written(&transitions), value(Scalar::Num(0.5)));
        assert!(transitions.advance(&surface, 11.0, true));
        assert_eq!(written(&transitions), value(Scalar::Num(0.25)));
        assert!(transitions.advance(&surface, 12.0, true));
        assert_eq!(written(&transitions), value(Scalar::Num(0.0)));
        assert!(!transitions.active());
        assert!(!transitions.advance(&surface, 13.0, true));
    }

    #[test]
    fn a_fresh_surface_eases_arrays_in_from_their_declared_start() {
        let surface = surface(
            SurfaceValue::Vector(vec![0.0, 0.0]),
            linear(0.25, Some(SurfaceValue::Vector(vec![0.0, 6.0]))),
        );
        let mut transitions = SurfaceTransitions::default();
        assert!(transitions.advance(&surface, 1.0, true));
        assert_eq!(
            written(&transitions),
            value(Scalar::Json(json!([0.0, 6.0])))
        );
        assert!(transitions.active());
        transitions.advance(&surface, 1.125, true);
        assert_eq!(
            written(&transitions),
            value(Scalar::Json(json!([0.0, 3.0])))
        );
        transitions.advance(&surface, 1.25, true);
        assert_eq!(
            written(&transitions),
            value(Scalar::Json(json!([0.0, 0.0])))
        );
        assert!(!transitions.active());
    }

    #[test]
    fn world_backdrop_reads_reserved_bindings_at_their_displayed_values() {
        use ui::mod_panel::backdrop::{WORLD_BLUR_BINDING, WORLD_BRIGHTNESS_BINDING};
        let mut surface = surface(SurfaceValue::Number(0.0), linear(1.0, None));
        surface
            .bindings
            .insert(WORLD_BLUR_BINDING.into(), SurfaceValue::Number(3.0));
        surface
            .bindings
            .insert(WORLD_BRIGHTNESS_BINDING.into(), SurfaceValue::Number(0.62));
        surface.transitions.insert(
            WORLD_BLUR_BINDING.into(),
            linear(0.25, Some(SurfaceValue::Number(0.0))),
        );
        let mut transitions = SurfaceTransitions::default();
        assert!(transitions.world_backdrop(&surface).blur == 3.0);
        transitions.advance(&surface, 1.0, true);
        let opening = transitions.world_backdrop(&surface);
        assert_eq!((opening.blur, opening.brightness), (0.0, 0.62));
        transitions.advance(&surface, 1.125, true);
        assert_eq!(transitions.world_backdrop(&surface).blur, 1.5);
        transitions.advance(&surface, 1.25, true);
        let open = transitions.world_backdrop(&surface);
        assert_eq!((open.blur, open.saturation), (3.0, 1.0));
    }

    #[test]
    fn mismatched_shapes_and_text_values_change_at_once() {
        let mut surface = surface(
            SurfaceValue::Number(1.0),
            linear(1.0, Some(SurfaceValue::Vector(vec![0.0, 0.0]))),
        );
        let mut transitions = SurfaceTransitions::default();
        transitions.advance(&surface, 0.0, true);
        assert_eq!(written(&transitions), value(Scalar::Num(1.0)));

        surface
            .bindings
            .insert("#value".into(), SurfaceValue::Vector(vec![4.0, 2.0]));
        transitions.advance(&surface, 0.1, true);
        assert_eq!(
            written(&transitions),
            value(Scalar::Json(json!([4.0, 2.0])))
        );

        surface
            .bindings
            .insert("#value".into(), SurfaceValue::Text("idle".into()));
        transitions.advance(&surface, 0.2, true);
        assert_eq!(written(&transitions), DataSource::new());
        assert!(!transitions.active());
    }

    #[test]
    fn exits_ease_to_their_declared_value_and_reopening_continues_from_the_display() {
        let surface = exiting(SurfaceValue::Number(0.0));
        let mut transitions = SurfaceTransitions::default();
        transitions.advance(&surface, 1.0, true);
        assert!(transitions.begin_exit(&surface));
        assert!(transitions.advance(&surface, 1.0, true));
        assert_eq!(written(&transitions), value(Scalar::Num(1.0)));
        transitions.advance(&surface, 1.5, true);
        assert_eq!(written(&transitions), value(Scalar::Num(0.5)));

        transitions.cancel_exit();
        assert!(transitions.advance(&surface, 1.5, true));
        assert_eq!(written(&transitions), value(Scalar::Num(0.5)));
        transitions.advance(&surface, 2.0, true);
        assert_eq!(written(&transitions), value(Scalar::Num(0.75)));

        assert!(transitions.begin_exit(&surface));
        transitions.advance(&surface, 2.0, true);
        assert!(transitions.active());
        transitions.advance(&surface, 3.0, true);
        assert_eq!(written(&transitions), value(Scalar::Num(0.0)));
        assert!(!transitions.active());
        assert!(transitions.exiting());
    }

    #[test]
    fn exits_that_would_not_move_report_nothing_to_animate() {
        let mut unpainted = SurfaceTransitions::default();
        assert!(!unpainted.begin_exit(&exiting(SurfaceValue::Number(0.0))));
        for surface in [
            surface(SurfaceValue::Number(1.0), linear(1.0, None)),
            exiting(SurfaceValue::Number(1.0)),
            exiting(SurfaceValue::Vector(vec![0.0, 0.0])),
            {
                let mut instant = exiting(SurfaceValue::Number(0.0));
                instant.transitions.get_mut("#value").unwrap().duration = 0.0;
                instant
            },
        ] {
            let mut transitions = SurfaceTransitions::default();
            transitions.advance(&surface, 1.0, true);
            assert!(!transitions.begin_exit(&surface));
            assert!(!transitions.exiting());
        }
    }

    #[test]
    fn without_animation_values_change_at_once_and_moving_values_settle() {
        let mut surface = surface(
            SurfaceValue::Number(1.0),
            linear(1.0, Some(SurfaceValue::Number(0.0))),
        );
        let mut transitions = SurfaceTransitions::default();
        assert!(transitions.advance(&surface, 1.0, false));
        assert_eq!(written(&transitions), value(Scalar::Num(1.0)));
        assert!(!transitions.active());

        surface
            .bindings
            .insert("#value".into(), SurfaceValue::Number(3.0));
        transitions.advance(&surface, 1.0, true);
        transitions.advance(&surface, 1.5, true);
        assert_eq!(written(&transitions), value(Scalar::Num(2.0)));
        assert!(transitions.advance(&surface, 1.6, false));
        assert_eq!(written(&transitions), value(Scalar::Num(3.0)));
        assert!(!transitions.active());

        surface
            .bindings
            .insert("#value".into(), SurfaceValue::Number(5.0));
        assert!(transitions.advance(&surface, 1.7, false));
        assert_eq!(written(&transitions), value(Scalar::Num(5.0)));
        assert!(!transitions.active());
    }
}

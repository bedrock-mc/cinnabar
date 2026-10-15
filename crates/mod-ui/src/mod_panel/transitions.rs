//! Eased presentation of surface bindings that declare transitions, driven by the caller's clock.

use std::collections::HashMap;

use json_ui::{DataSource, Scalar};
use serde_json::json;
use ui::mod_panel::{Surface, SurfaceValue, TransitionEasing};

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
    /// The displayed components at `now`, exactly `to` once the duration has elapsed.
    fn sample(&self, now: f64) -> Vec<f64> {
        let t = if self.duration > 0.0 {
            (now - self.began) / self.duration
        } else {
            1.0
        };
        let progress = self.easing.progress(t);
        self.from
            .iter()
            .zip(&self.to)
            .map(|(from, to)| from + (to - from) * progress)
            .collect()
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
}

impl SurfaceTransitions {
    /// Retargets changed bindings from their displayed values and reports whether any
    /// displayed value differs from the last [`Self::write`].
    pub(crate) fn advance(&mut self, surface: &Surface, now: f64) -> bool {
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
        for (name, transition) in &surface.transitions {
            let Some(binding) = surface.bindings.get(name) else {
                continue;
            };
            let Some(target) = binding.numbers() else {
                continue;
            };
            let retarget = |from: Vec<f64>| Tween {
                from,
                to: target.to_vec(),
                began: now,
                duration: transition.duration,
                easing: transition.easing,
                vector: matches!(binding, SurfaceValue::Vector(_)),
            };
            match self.tweens.get_mut(name) {
                None => {
                    let from = transition
                        .from
                        .as_ref()
                        .and_then(SurfaceValue::numbers)
                        .filter(|from| from.len() == target.len())
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
            screen: "fixture.root".into(),
            document: String::new(),
            bindings: BTreeMap::from([("#value".into(), value)]),
            transitions: BTreeMap::from([("#value".into(), transition)]),
        }
    }

    fn linear(duration: f64, from: Option<SurfaceValue>) -> SurfaceTransition {
        SurfaceTransition {
            duration,
            easing: TransitionEasing::Named(Easing::Linear),
            from,
        }
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
        assert!(transitions.advance(&surface, 10.0));
        assert_eq!(written(&transitions), value(Scalar::Num(0.0)));
        assert!(!transitions.active());

        surface
            .bindings
            .insert("#value".into(), SurfaceValue::Number(1.0));
        assert!(transitions.advance(&surface, 10.0));
        assert!(transitions.advance(&surface, 10.5));
        assert_eq!(written(&transitions), value(Scalar::Num(0.5)));

        surface
            .bindings
            .insert("#value".into(), SurfaceValue::Number(0.0));
        assert!(transitions.advance(&surface, 10.5));
        assert_eq!(written(&transitions), value(Scalar::Num(0.5)));
        assert!(transitions.advance(&surface, 11.0));
        assert_eq!(written(&transitions), value(Scalar::Num(0.25)));
        assert!(transitions.advance(&surface, 12.0));
        assert_eq!(written(&transitions), value(Scalar::Num(0.0)));
        assert!(!transitions.active());
        assert!(!transitions.advance(&surface, 13.0));
    }

    #[test]
    fn a_fresh_surface_eases_arrays_in_from_their_declared_start() {
        let surface = surface(
            SurfaceValue::Vector(vec![0.0, 0.0]),
            linear(0.25, Some(SurfaceValue::Vector(vec![0.0, 6.0]))),
        );
        let mut transitions = SurfaceTransitions::default();
        assert!(transitions.advance(&surface, 1.0));
        assert_eq!(
            written(&transitions),
            value(Scalar::Json(json!([0.0, 6.0])))
        );
        assert!(transitions.active());
        transitions.advance(&surface, 1.125);
        assert_eq!(
            written(&transitions),
            value(Scalar::Json(json!([0.0, 3.0])))
        );
        transitions.advance(&surface, 1.25);
        assert_eq!(
            written(&transitions),
            value(Scalar::Json(json!([0.0, 0.0])))
        );
        assert!(!transitions.active());
    }

    #[test]
    fn mismatched_shapes_and_text_values_change_at_once() {
        let mut surface = surface(
            SurfaceValue::Number(1.0),
            linear(1.0, Some(SurfaceValue::Vector(vec![0.0, 0.0]))),
        );
        let mut transitions = SurfaceTransitions::default();
        transitions.advance(&surface, 0.0);
        assert_eq!(written(&transitions), value(Scalar::Num(1.0)));

        surface
            .bindings
            .insert("#value".into(), SurfaceValue::Vector(vec![4.0, 2.0]));
        transitions.advance(&surface, 0.1);
        assert_eq!(
            written(&transitions),
            value(Scalar::Json(json!([4.0, 2.0])))
        );

        surface
            .bindings
            .insert("#value".into(), SurfaceValue::Text("idle".into()));
        transitions.advance(&surface, 0.2);
        assert_eq!(written(&transitions), DataSource::new());
        assert!(!transitions.active());
    }
}

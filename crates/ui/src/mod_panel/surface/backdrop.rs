//! Glass panels, the world backdrop and element filters: mod-surface extensions that blur
//! the 3-D scene or a control's own drawing.
//!
//! Neither reaches server or resource-pack JSON-UI: only mod surfaces may name
//! [`GLASS_RENDERER`], and only their hosts read the reserved world bindings.

use json_ui::filter::{BLUR_BINDING, FILTER_PROPERTY, MAX_BLUR as MAX_FILTER_BLUR};
use serde_json::{Map, Value};

use super::{SurfaceValue, bounded};

/// Custom renderer for a rounded panel showing the blurred scene behind it.
pub const GLASS_RENDERER: &str = "cinnabar_glass";
/// Largest backdrop blur, a Gaussian standard deviation in GUI pixels like CSS `blur()`.
pub const MAX_BACKDROP_BLUR: f64 = 32.0;
/// Largest saturation or brightness factor; zero is the lower bound of both.
pub const MAX_BACKDROP_FACTOR: f64 = render_api::UI_BACKDROP_MAX_FACTOR as f64;

/// Surface binding that blurs the world behind an open panel, in GUI pixels.
pub const WORLD_BLUR_BINDING: &str = "#world_blur";
/// Surface binding that scales the brightness of the world behind an open panel.
pub const WORLD_BRIGHTNESS_BINDING: &str = "#world_brightness";
/// Surface binding that scales the saturation of the world behind an open panel.
pub const WORLD_SATURATION_BINDING: &str = "#world_saturation";

/// One bounded backdrop property: its authored and bound names, identity and maximum.
#[derive(Clone, Copy, Debug)]
pub struct BackdropProperty {
    /// Authored property on a glass node.
    pub name: &'static str,
    /// The `#name` a binding override writes, which wins over the authored value.
    pub bound: &'static str,
    /// The reserved world-backdrop binding for this property.
    pub world: &'static str,
    /// The value that leaves the scene unchanged.
    pub identity: f64,
    pub max: f64,
}

impl BackdropProperty {
    /// Whether `value` lies within `0..=max`.
    pub fn accepts(self, value: f64) -> bool {
        value.is_finite() && (0.0..=self.max).contains(&value)
    }

    /// `value` clamped into range, or the identity when it is not a finite number.
    pub fn clamp(self, value: Option<f64>) -> f64 {
        value
            .filter(|value| value.is_finite())
            .map_or(self.identity, |value| value.clamp(0.0, self.max))
    }
}

/// Gaussian blur in GUI pixels.
pub const BLUR: BackdropProperty = BackdropProperty {
    name: "blur",
    bound: "#blur",
    world: WORLD_BLUR_BINDING,
    identity: 0.0,
    max: MAX_BACKDROP_BLUR,
};
/// CSS `saturate()` factor.
pub const SATURATION: BackdropProperty = BackdropProperty {
    name: "saturation",
    bound: "#saturation",
    world: WORLD_SATURATION_BINDING,
    identity: 1.0,
    max: MAX_BACKDROP_FACTOR,
};
/// CSS `brightness()` factor.
pub const BRIGHTNESS: BackdropProperty = BackdropProperty {
    name: "brightness",
    bound: "#brightness",
    world: WORLD_BRIGHTNESS_BINDING,
    identity: 1.0,
    max: MAX_BACKDROP_FACTOR,
};
/// Every backdrop property, in [`render_api::UiBackdropPaint`] field order.
pub const BACKDROP_PROPERTIES: [BackdropProperty; 3] = [BLUR, SATURATION, BRIGHTNESS];

/// Rejects an authored glass value outside its bound; bound values are clamped when drawn.
pub(super) fn validate_glass(object: &Map<String, Value>) -> Result<(), String> {
    for property in BACKDROP_PROPERTIES {
        if let Some(value) = object.get(property.name)
            && !value.as_f64().is_some_and(|value| property.accepts(value))
        {
            return Err(format!(
                "surface glass {} must be a number from 0 to {}",
                property.name, property.max
            ));
        }
    }
    Ok(())
}

/// Checks one control's filter and returns whether its subtree is filtered. `filtered`
/// reports a filtering ancestor: filters do not nest.
pub(super) fn filter(object: &Map<String, Value>, filtered: bool) -> Result<bool, String> {
    let declared = object.get(FILTER_PROPERTY);
    let bound = super::paint_transform::bound_targets(object).contains(&BLUR_BINDING);
    if declared.is_none() {
        if bound {
            return Err(format!(
                "surface blur bindings require a {FILTER_PROPERTY} property"
            ));
        }
        return Ok(filtered);
    }
    if filtered {
        return Err("surface filters cannot nest".into());
    }
    let fields = declared
        .and_then(Value::as_object)
        .ok_or("surface filter must be an object")?;
    for (name, value) in fields {
        if name != "blur" {
            return Err(format!("surface {FILTER_PROPERTY} field is unsupported"));
        }
        if !value
            .as_f64()
            .is_some_and(|blur| blur.is_finite() && (0.0..=MAX_FILTER_BLUR).contains(&blur))
        {
            return Err(format!(
                "surface {FILTER_PROPERTY} blur must be a number from 0 to {MAX_FILTER_BLUR}"
            ));
        }
    }
    Ok(true)
}

/// Rejects a reserved world-backdrop binding that is not a number within its bound.
pub(super) fn validate_world_binding(name: &str, value: &SurfaceValue) -> Result<(), String> {
    let Some(property) = BACKDROP_PROPERTIES
        .iter()
        .find(|property| property.world == name)
    else {
        return Ok(());
    };
    match value {
        SurfaceValue::Number(number) if bounded(number) && property.accepts(*number) => Ok(()),
        _ => Err(format!(
            "surface binding {name} must be a number from 0 to {}",
            property.max
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mod_panel::Surface;
    use std::collections::BTreeMap;

    /// A one-node surface whose glass node carries `glass` properties.
    fn surface(glass: Value, bindings: BTreeMap<String, SurfaceValue>) -> Surface {
        let mut node =
            serde_json::json!({"type":"custom","renderer":GLASS_RENDERER,"size":[40,20]});
        node.as_object_mut()
            .unwrap()
            .extend(glass.as_object().unwrap().clone());
        Surface {
            collections: Default::default(),
            screen: "glass.root".into(),
            document: serde_json::json!({"namespace":"glass","root":{
                "type":"panel","controls":[{"pane":node}]
            }})
            .to_string(),
            bindings,
            transitions: Default::default(),
        }
    }

    #[test]
    fn glass_accepts_bounded_literals_and_rejects_values_beyond_them() {
        let valid = serde_json::json!({"blur":26,"saturation":1.12,"brightness":0,
            "radius":8,"color":[0.04,0.07,0.1,0.78],"highlight_color":[0.94,0.96,1,0.14]});
        surface(valid, BTreeMap::new()).validate().unwrap();
        for invalid in [
            serde_json::json!({"blur":-1}),
            serde_json::json!({"blur":32.5}),
            serde_json::json!({"saturation":2.5}),
            serde_json::json!({"brightness":"dim"}),
        ] {
            assert!(
                surface(invalid.clone(), BTreeMap::new())
                    .validate()
                    .is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn reserved_world_bindings_must_be_bounded_numbers() {
        let bindings = |name: &str, value| BTreeMap::from([(name.to_owned(), value)]);
        let glass = serde_json::json!({});
        for (name, value) in [
            (WORLD_BLUR_BINDING, SurfaceValue::Number(3.0)),
            (WORLD_BRIGHTNESS_BINDING, SurfaceValue::Number(0.62)),
            (WORLD_SATURATION_BINDING, SurfaceValue::Number(2.0)),
            ("#world_blurry", SurfaceValue::Text("other".into())),
        ] {
            surface(glass.clone(), bindings(name, value))
                .validate()
                .unwrap();
        }
        for (name, value) in [
            (WORLD_BLUR_BINDING, SurfaceValue::Number(33.0)),
            (WORLD_BRIGHTNESS_BINDING, SurfaceValue::Number(-0.1)),
            (
                WORLD_SATURATION_BINDING,
                SurfaceValue::Vector(vec![1.0, 1.0]),
            ),
            (WORLD_BLUR_BINDING, SurfaceValue::Bool(true)),
        ] {
            assert!(
                surface(glass.clone(), bindings(name, value))
                    .validate()
                    .is_err(),
                "{name}"
            );
        }
    }

    /// A surface whose root panel holds `pane`.
    fn holding(pane: Value) -> Surface {
        Surface {
            collections: Default::default(),
            screen: "glass.root".into(),
            document: serde_json::json!({"namespace":"glass","root":{
                "type":"panel","controls":[{"pane":pane}]
            }})
            .to_string(),
            bindings: BTreeMap::new(),
            transitions: Default::default(),
        }
    }

    #[test]
    fn filters_accept_one_bounded_blur_and_never_nest() {
        let label = serde_json::json!({"type":"label","text":"hi"});
        let filtered = |filter: Value, child: Value| {
            serde_json::json!({"type":"panel","cinnabar_filter":filter,
                "controls":[{"child":child}]})
        };
        holding(filtered(serde_json::json!({"blur":5}), label.clone()))
            .validate()
            .unwrap();
        holding(serde_json::json!({"type":"panel","cinnabar_filter":{},
            "bindings":[{"binding_name":"#entering","binding_name_override":"#cinnabar_blur"}]}))
        .validate()
        .unwrap();
        for invalid in [
            filtered(serde_json::json!({"blur":17}), label.clone()),
            filtered(serde_json::json!({"blur":2,"spread":1}), label.clone()),
            filtered(serde_json::json!(5), label.clone()),
            filtered(
                serde_json::json!({"blur":2}),
                serde_json::json!({"type":"panel","cinnabar_filter":{"blur":1}}),
            ),
            serde_json::json!({"type":"panel",
                "bindings":[{"binding_name":"#entering","binding_name_override":"#cinnabar_blur"}]}),
        ] {
            assert!(holding(invalid.clone()).validate().is_err(), "{invalid}");
        }
    }

    #[test]
    fn drawn_values_clamp_into_their_bounds() {
        assert_eq!(BLUR.clamp(Some(40.0)), MAX_BACKDROP_BLUR);
        assert_eq!(SATURATION.clamp(Some(f64::NAN)), 1.0);
        assert_eq!(BRIGHTNESS.clamp(None), 1.0);
        assert_eq!(BRIGHTNESS.clamp(Some(-1.0)), 0.0);
    }
}

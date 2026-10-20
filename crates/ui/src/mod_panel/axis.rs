//! Shared syntax and bounds for authored absolute and relative single-axis routes.

use super::{SurfaceValue, surface::binding_name};
use std::collections::BTreeMap;

/// Authored travel or a changing scalar sampled when a pointer gesture begins.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AxisExtent<'a> {
    Literal(f64),
    Binding(&'a str),
}

impl AxisExtent<'_> {
    /// Resolves a positive bounded extent without changing an active gesture's gain.
    pub fn resolve(self, bindings: &BTreeMap<String, SurfaceValue>) -> Option<f64> {
        let value = match self {
            Self::Literal(value) => value,
            Self::Binding(name) => match bindings.get(name)? {
                SurfaceValue::Number(value) => *value,
                _ => return None,
            },
        };
        (value > 0.0 && SurfaceValue::Number(value).validate().is_ok()).then_some(value)
    }
}

/// One slider driven by an authored pointer axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisRoute<'a> {
    pub control: usize,
    /// Zero for horizontal input, one for vertical input.
    pub axis: usize,
    pub relative: bool,
    /// Optional authored travel replacing the full reference extent for a relative route.
    pub extent: Option<AxisExtent<'a>>,
}

impl<'a> AxisRoute<'a> {
    /// Parses only bounded axis routes; relative travel is finite, positive and optional.
    pub fn parse(action: &'a str) -> Option<Self> {
        let (route, relative) = action
            .strip_prefix("mod.axis:")
            .map(|route| (route, false))
            .or_else(|| {
                action
                    .strip_prefix("mod.drag_axis:")
                    .map(|route| (route, true))
            })?;
        let mut parts = route.split(':');
        let control = parts.next()?.parse().ok()?;
        let axis = match parts.next()? {
            "x" => 0,
            "y" => 1,
            _ => return None,
        };
        let extent = match parts.next() {
            Some(part) => Some({
                if binding_name(part) {
                    AxisExtent::Binding(part)
                } else {
                    AxisExtent::Literal(part.parse::<f64>().ok()?)
                }
            }),
            None => None,
        };
        if parts.next().is_some()
            || extent.is_some_and(|extent| {
                !relative
                    || matches!(extent, AxisExtent::Literal(value)
                    if value <= 0.0 || SurfaceValue::Number(value).validate().is_err())
            })
        {
            return None;
        }
        Some(Self {
            control,
            axis,
            relative,
            extent,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_routes_accept_optional_relative_extent_and_reject_unbounded_or_ambiguous_forms() {
        assert_eq!(
            AxisRoute::parse("mod.drag_axis:3:x:112"),
            Some(AxisRoute {
                control: 3,
                axis: 0,
                relative: true,
                extent: Some(AxisExtent::Literal(112.0))
            })
        );
        assert!(AxisRoute::parse("mod.axis:3:y").is_some());
        assert!(AxisRoute::parse("mod.drag_axis:3:y").is_some());
        let bound = AxisRoute::parse("mod.drag_axis:3:x:#resize_extent").unwrap();
        let mut bindings = BTreeMap::from([("#resize_extent".into(), SurfaceValue::Number(112.0))]);
        assert_eq!(bound.extent.unwrap().resolve(&bindings), Some(112.0));
        bindings.insert("#resize_extent".into(), SurfaceValue::Number(0.0));
        assert_eq!(bound.extent.unwrap().resolve(&bindings), None);
        for action in [
            "mod.drag_axis:3:z",
            "mod.axis:3:x:112",
            "mod.drag_axis:3:x:0",
            "mod.drag_axis:3:x:-1",
            "mod.drag_axis:3:x:NaN",
            "mod.drag_axis:3:x:inf",
            "mod.drag_axis:3:x:1000001",
            "mod.drag_axis:3:x:112:more",
            "mod.axis:3:x:#resize_extent",
            "mod.drag_axis:3:x:#bad/name",
        ] {
            assert!(AxisRoute::parse(action).is_none(), "{action}");
        }
    }
}

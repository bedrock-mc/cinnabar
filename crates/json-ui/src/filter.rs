//! Cinnabar paint-time filters: a control declaring [`FILTER_PROPERTY`] blurs its own draws and
//! every descendant's together, like CSS `filter: blur()`, without changing layout. Like paint
//! transforms, only catalogs opted in with [`crate::Catalog::with_paint_transforms`] keep the
//! property, so resource packs and server JSON-UI paint exactly as before.

use std::sync::Arc;

use serde_json::Value;

/// The control property a mod surface authors: `{"blur"}`.
pub const FILTER_PROPERTY: &str = "cinnabar_filter";
/// Binding target replacing the blur.
pub const BLUR_BINDING: &str = "#cinnabar_blur";
/// Largest blur, a Gaussian standard deviation in layout units.
pub const MAX_BLUR: f64 = 16.0;

/// One control's authored filter, clamped to its bounds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ControlFilter {
    /// Gaussian standard deviation in layout units.
    pub blur: f64,
}

impl ControlFilter {
    /// Reads an object property; a missing, malformed or non-finite blur keeps zero and a
    /// finite one clamps to `0..=MAX_BLUR`.
    pub fn parse(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let blur = object
            .get("blur")
            .and_then(Value::as_f64)
            .filter(|blur| blur.is_finite())
            .map_or(0.0, |blur| blur.clamp(0.0, MAX_BLUR));
        Some(Self { blur })
    }

    /// Whether the filter changes nothing it draws.
    pub fn is_identity(&self) -> bool {
        self.blur <= 0.0
    }
}

/// The filtered subtree a draw belongs to: the declaring control's key and its filter. Every
/// draw of that control and its descendants carries the same group.
#[derive(Clone, Debug, PartialEq)]
pub struct FilterGroup {
    pub owner: Arc<str>,
    pub filter: ControlFilter,
}

impl FilterGroup {
    /// The group `control_key` opens for its subtree, or `inherited` when its own filter is
    /// absent or the identity. An inner filter replaces an outer one; surfaces reject nesting.
    pub(crate) fn place(
        inherited: Option<&Arc<FilterGroup>>,
        own: Option<ControlFilter>,
        control_key: &str,
    ) -> Option<Arc<FilterGroup>> {
        match own.filter(|own| !own.is_identity()) {
            Some(filter) => Some(Arc::new(FilterGroup {
                owner: control_key.into(),
                filter,
            })),
            None => inherited.cloned(),
        }
    }
}

/// Writes a bound blur into the control's filter object; a control that does not declare
/// [`FILTER_PROPERTY`] ignores the target.
pub(crate) fn bind(value: &Value, current: Option<&Value>) -> Option<Value> {
    let mut object = current?.as_object()?.clone();
    object.insert("blur".to_owned(), value.clone());
    Some(Value::Object(object))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn filters_clamp_authored_blur_and_skip_malformed_values() {
        let parse = |value| ControlFilter::parse(&value);
        assert_eq!(parse(json!({"blur": 5})).unwrap().blur, 5.0);
        assert_eq!(parse(json!({"blur": 99})).unwrap().blur, MAX_BLUR);
        assert_eq!(parse(json!({"blur": -1})).unwrap().blur, 0.0);
        assert!(parse(json!({"blur": "soft"})).unwrap().is_identity());
        assert_eq!(parse(json!(5)), None);
    }

    #[test]
    fn subtrees_inherit_the_nearest_non_identity_filter() {
        let outer = FilterGroup::place(None, Some(ControlFilter { blur: 3.0 }), "a").unwrap();
        assert_eq!(&*outer.owner, "a");
        let inherited = FilterGroup::place(Some(&outer), None, "a/b").unwrap();
        assert!(Arc::ptr_eq(&inherited, &outer));
        let identity = FilterGroup::place(Some(&outer), Some(ControlFilter::default()), "a/c");
        assert!(Arc::ptr_eq(&identity.unwrap(), &outer));
        assert_eq!(
            FilterGroup::place(None, Some(ControlFilter::default()), "d"),
            None
        );
        assert_eq!(
            bind(&json!(4), Some(&json!({"blur": 1}))),
            Some(json!({"blur": 4}))
        );
        assert_eq!(bind(&json!(4), None), None);
    }
}

//! Compile authored locator forms before any frame consumes the geometry.
use super::*;
use assets::EntityGeometryLocator;

/// Accepts vector and object locators with native zero defaults and bounded scalar validation.
pub(super) fn parse(
    value: Option<&Value>,
    path: &Path,
) -> Result<Box<[EntityGeometryLocator]>, AssetError> {
    let Some(value) = value else {
        return Ok(Box::default());
    };
    let object = value
        .as_object()
        .ok_or_else(|| invalid("entity locators must be an object"))?;
    if object.len() > assets::MAX_GEOMETRY_LOCATORS {
        return Err(invalid("entity geometry locator count exceeds bound"));
    }
    let zero = [zero_scalar(); 3];
    object
        .iter()
        .map(|(name, value)| {
            if name.is_empty()
                || name.len() > MAX_ENTITY_GEOMETRY_NAME_BYTES
                || name.chars().any(char::is_control)
            {
                return Err(invalid("invalid entity geometry locator name"));
            }
            let (offset, rotation, ignore_inherited_scale) = if value.is_array() {
                let wrapper = serde_json::json!({"offset":value});
                (required_vec(&wrapper, "offset", path)?, zero, false)
            } else {
                validate_object_fields(
                    value,
                    path,
                    &["offset", "rotation", "ignore_inherited_scale"],
                    &[],
                )?;
                (
                    optional_vec(value, "offset", path)?.unwrap_or(zero),
                    optional_vec(value, "rotation", path)?.unwrap_or(zero),
                    optional_bool(value, "ignore_inherited_scale", path)?.unwrap_or(false),
                )
            };
            Ok(EntityGeometryLocator {
                name: name.clone().into_boxed_str(),
                offset,
                rotation,
                ignore_inherited_scale,
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locator_forms_keep_offset_rotation_and_scale_policy() {
        let value = serde_json::json!({"wing":[1,2,3],"hand":{"offset":[4,5,6],"rotation":[0,90,0],"ignore_inherited_scale":true}});
        let parsed = parse(Some(&value), Path::new("model.json")).unwrap();
        assert_eq!(
            parsed[0].offset.map(EntityGeometryScalar::get),
            [4.0, 5.0, 6.0]
        );
        assert_eq!(
            parsed[0].rotation.map(EntityGeometryScalar::get),
            [0.0, 90.0, 0.0]
        );
        assert!(parsed[0].ignore_inherited_scale);
        assert_eq!(
            parsed[1].offset.map(EntityGeometryScalar::get),
            [1.0, 2.0, 3.0]
        );
        assert!(!parsed[1].ignore_inherited_scale);
        assert!(
            parse(
                Some(&serde_json::json!({"bad":{"offset":[1,2]}})),
                Path::new("model.json")
            )
            .is_err()
        );
    }
}

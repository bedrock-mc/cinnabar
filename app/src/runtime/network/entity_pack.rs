//! Behavior-pack property defaults for the client world.

use pack_runtime::pack_textures::parse_pack_json;
use resource_pack::LayeredPackView;
use std::sync::Arc;

/// Property defaults of each `entities/*.json` behavior definition the stack carries, keyed by
/// entity type. Only the winning copy of a file is read; a malformed property is skipped.
pub(super) fn pack_property_defaults(
    view: &LayeredPackView,
) -> Vec<(Arc<str>, Vec<client_world::PropertyDefault>)> {
    view.winning_files("entities/", "json")
        .into_iter()
        .filter_map(|(_, bytes)| {
            let root = parse_pack_json(&bytes)?;
            let description = &root["minecraft:entity"]["description"];
            let identifier: Arc<str> = description["identifier"].as_str()?.into();
            let defaults = description["properties"]
                .as_object()?
                .iter()
                .filter_map(|(name, definition)| property_default(name, definition))
                .collect::<Vec<_>>();
            (!defaults.is_empty()).then_some((identifier, defaults))
        })
        .collect()
}

fn property_default(
    name: &str,
    definition: &serde_json::Value,
) -> Option<client_world::PropertyDefault> {
    let mut values = None;
    let default = &definition["default"];
    let number = match definition["type"].as_str()? {
        "bool" => f32::from(u8::from(default.as_bool()?)),
        "int" | "float" => default.as_f64()? as f32,
        "enum" => {
            let names = definition["values"]
                .as_array()?
                .iter()
                .map(|value| value.as_str().map(Arc::<str>::from))
                .collect::<Option<Vec<_>>>()?;
            let wanted = default.as_str()?;
            let index = names.iter().position(|value| value.as_ref() == wanted)?;
            values = Some(names.into());
            index as f32
        }
        _ => return None,
    };
    number.is_finite().then(|| client_world::PropertyDefault {
        name: name.into(),
        values,
        default: number,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn property_defaults_read_bool_number_and_enum_index() {
        use super::property_default;
        let json = |text: &str| serde_json::from_str::<serde_json::Value>(text).unwrap();
        let flag = property_default("a", &json(r#"{"type":"bool","default":true}"#)).unwrap();
        assert_eq!(flag.default, 1.0);
        let level = property_default("b", &json(r#"{"type":"int","default":4}"#)).unwrap();
        assert_eq!(level.default, 4.0);
        let variant = property_default(
            "c",
            &json(r#"{"type":"enum","values":["x","y"],"default":"y"}"#),
        )
        .unwrap();
        assert_eq!((variant.default, variant.values.unwrap().len()), (1.0, 2));
        assert!(property_default("d", &json(r#"{"type":"weird"}"#)).is_none());
    }

    #[test]
    fn review_property_defaults_skip_invalid_types_membership_and_nonfinite_values() {
        for definition in [
            serde_json::json!({"type":"bool", "default":"invalid"}),
            serde_json::json!({"type":"float", "default":1e100}),
            serde_json::json!({"type":"float", "default":"invalid"}),
            serde_json::json!({"type":"int", "default":"invalid"}),
            serde_json::json!({"type":"enum", "values":["x"], "default":"missing"}),
            serde_json::json!({"type":"enum", "values":[4,"x"], "default":"x"}),
        ] {
            assert!(
                super::property_default("test", &definition).is_none(),
                "{definition}"
            );
        }
    }
}

#[cfg(all(test, feature = "reports"))]
mod pack_report;

#[cfg(all(test, feature = "reports"))]
mod equipment_report;

#[cfg(all(test, feature = "reports"))]
mod render_report;

#[cfg(all(test, unix, feature = "reports"))]
mod lobby_bench;

#[cfg(all(test, feature = "reports"))]
mod scene_report;

#[cfg(all(test, feature = "reports"))]
mod projectile_report;

#[cfg(all(test, feature = "reports"))]
mod mob_motion_report;

//! Authored food extraction from the installed behavior pack.
use super::*;
use assets::CompiledItemFood;

/// Retains explicit food values; malformed or unsupported definitions remain unavailable.
pub fn compile_item_foods(behavior_pack: &Path) -> Result<Vec<CompiledItemFood>, AssetError> {
    Ok(item_attack::components(behavior_pack)?
        .into_iter()
        .filter_map(|(identifier, components)| {
            let food = components.get("minecraft:food")?;
            let nutrition = u32::try_from(food.get("nutrition")?.as_u64()?).ok()?;
            let modifier = saturation_modifier(food.get("saturation_modifier")?)?;
            Some(CompiledItemFood {
                identifier,
                nutrition,
                saturation_modifier: ItemDisplayScalar::new(modifier)?,
            })
        })
        .collect())
}

/// Reads numeric modifiers and the legacy named component values still present in vanilla packs.
fn saturation_modifier(value: &Value) -> Option<f32> {
    let result = if let Some(value) = value.as_f64() {
        value as f32
    } else {
        match value.as_str()? {
            "poor" => 0.1,
            "low" => 0.3,
            "normal" => 0.6,
            "good" => 0.8,
            "max" => 1.0,
            "supernatural" => 1.2,
            _ => return None,
        }
    };
    (result.is_finite() && result >= 0.0).then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_food_extraction_preserves_values_and_skips_malformed_siblings() {
        let root = std::env::temp_dir().join(format!("cinnabar-food-{}", std::process::id()));
        std::fs::create_dir_all(root.join("items")).unwrap();
        for (file, id, food) in [
            (
                "numeric",
                "fixture:a",
                serde_json::json!({"nutrition":7,"saturation_modifier":0.25}),
            ),
            (
                "legacy",
                "fixture:b",
                serde_json::json!({"nutrition":2,"saturation_modifier":"poor"}),
            ),
            (
                "bad",
                "fixture:c",
                serde_json::json!({"nutrition":2,"saturation_modifier":"unknown"}),
            ),
            ("missing", "fixture:d", serde_json::json!({"nutrition":2})),
        ] {
            let document = serde_json::json!({"minecraft:item":{"description":{"identifier":id},"components":{"minecraft:food":food}}});
            std::fs::write(
                root.join("items").join(format!("{file}.json")),
                document.to_string(),
            )
            .unwrap();
        }
        let entries = compile_item_foods(&root).unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].identifier.as_ref(), "fixture:a");
        assert_eq!(entries[0].nutrition, 7);
        assert_eq!(entries[0].saturation_modifier.get(), 0.25);
        assert_eq!(entries[1].identifier.as_ref(), "fixture:b");
        assert_eq!(entries[1].saturation_modifier.get(), 0.1);
    }

    #[test]
    fn numeric_and_legacy_modifiers_do_not_guess_unknown_names() {
        assert_eq!(saturation_modifier(&serde_json::json!(0.35)), Some(0.35));
        for (name, number) in [
            ("poor", 0.1),
            ("low", 0.3),
            ("normal", 0.6),
            ("good", 0.8),
            ("max", 1.0),
            ("supernatural", 1.2),
        ] {
            assert_eq!(saturation_modifier(&serde_json::json!(name)), Some(number));
        }
        assert_eq!(saturation_modifier(&serde_json::json!("made_up")), None);
        assert_eq!(saturation_modifier(&serde_json::json!(-1)), None);
    }
}

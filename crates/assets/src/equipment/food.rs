//! Installation-owned food facts carried beside equipment timing metadata.
use crate::{AssetError, ItemDisplayScalar};
use serde::{Deserialize, Serialize};

/// Exact nutrition and saturation component values for one item identifier.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledItemFood {
    pub identifier: Box<str>,
    pub nutrition: u32,
    pub saturation_modifier: ItemDisplayScalar,
}

/// Validates sorted bounded identifiers and finite nonnegative authored food values.
pub(super) fn validate(entries: &[CompiledItemFood]) -> Result<(), AssetError> {
    if entries.len() > super::MAX_ITEM_TIMINGS {
        return Err(super::invalid("too many food definitions"));
    }
    let mut previous: Option<&str> = None;
    for entry in entries {
        let id = entry.identifier.as_ref();
        if id.is_empty()
            || id.len() > super::MAX_EQUIPMENT_IDENTIFIER_BYTES
            || !id.contains(':')
            || previous.is_some_and(|value| value >= id)
            || entry.saturation_modifier.get() < 0.0
        {
            return Err(super::invalid("invalid or unordered food definition"));
        }
        previous = Some(id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        RuntimeEquipmentCatalog, encode_equipment_catalog_full, encode_equipment_catalog_with_food,
    };
    #[test]
    fn optional_food_roundtrips_and_old_carriers_stay_empty() {
        let old = encode_equipment_catalog_full([1; 32], [2; 32], &[], &[], &[]).unwrap();
        assert!(
            RuntimeEquipmentCatalog::decode(&old)
                .unwrap()
                .item_foods()
                .is_empty()
        );
        let food = CompiledItemFood {
            identifier: "fixture:food".into(),
            nutrition: 4,
            saturation_modifier: ItemDisplayScalar::new(0.3).unwrap(),
        };
        let bytes = encode_equipment_catalog_with_food(
            [1; 32],
            [2; 32],
            &[],
            &[],
            &[],
            &[],
            std::slice::from_ref(&food),
        )
        .unwrap();
        let catalog = RuntimeEquipmentCatalog::decode(&bytes).unwrap();
        assert_eq!(catalog.item_food("fixture:food"), Some(&food));
        assert!(catalog.item_food("fixture:unknown").is_none());
        let mut bad = food.clone();
        bad.saturation_modifier = ItemDisplayScalar::new(-0.3).unwrap();
        assert!(
            encode_equipment_catalog_with_food([1; 32], [2; 32], &[], &[], &[], &[], &[bad])
                .is_err()
        );
        assert!(
            encode_equipment_catalog_with_food(
                [1; 32],
                [2; 32],
                &[],
                &[],
                &[],
                &[],
                &[food.clone(), food]
            )
            .is_err()
        );
    }
}

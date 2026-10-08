use crate::{AssetError, Material};

/// Checks positional ranges before either serializing or publishing material tables.
pub(crate) fn validate(materials: &[Material]) -> Result<(), AssetError> {
    for material in materials {
        let weight = f32::from_bits(material.variation_weight);
        let start = material.variation_start as usize;
        let count = material.variation_count as usize;
        let valid = weight.is_finite() && (0.0..=1.0).contains(&weight);
        if !valid || (count == 0 && start != 0) {
            return Err(invalid());
        }
        if count != 0 {
            let end = start.checked_add(count).ok_or_else(invalid)?;
            let entries = materials.get(start..end).ok_or_else(invalid)?;
            if weight != 0.0
                || entries
                    .iter()
                    .any(|entry| entry.variation_count != 0 || entry.flags != material.flags)
            {
                return Err(invalid());
            }
            let total: f32 = entries
                .iter()
                .map(|entry| f32::from_bits(entry.variation_weight))
                .sum();
            if !total.is_finite() || (total - 1.0).abs() > 0.0001 {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

/// Names invalid selector metadata without exposing unchecked table entries.
fn invalid() -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: "invalid positional material range, weight or flags".into(),
    }
}

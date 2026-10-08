//! Pack-authored cube UV rotation and world-leaf ambient occlusion metadata.

use super::{BlockTextureMap, Value};
use assets::{
    AssetError, BlockFace, MATERIAL_FLAG_ISOTROPIC, MATERIAL_LEAF_AO_EXPONENT_MAX,
    MATERIAL_LEAF_AO_EXPONENT_SCALE, MATERIAL_LEAF_AO_EXPONENT_SHIFT, RegistryRecord,
    legacy_resource_pack_block_alias, material_leaf_ao_exponent,
};

impl BlockTextureMap {
    pub(crate) fn isotropic_face_flags(&self, record: &RegistryRecord) -> [u32; BlockFace::ALL.len()] {
        let name = record
            .name
            .strip_prefix("minecraft:")
            .unwrap_or(&record.name);
        let entry = self
            .entries
            .get(name)
            .or_else(|| self.entries.get(legacy_resource_pack_block_alias(name)?));
        let value = entry.and_then(|entry| entry.extra.get("isotropic"));
        BlockFace::ALL.map(|face| {
            if is_isotropic(value, face) {
                MATERIAL_FLAG_ISOTROPIC
            } else {
                0
            }
        })
    }

    pub(crate) fn leaf_world_face_flags(
        &self,
        record: &RegistryRecord,
    ) -> Result<[u32; BlockFace::ALL.len()], AssetError> {
        let name = record
            .name
            .strip_prefix("minecraft:")
            .unwrap_or(&record.name);
        let entry = self
            .entries
            .get(name)
            .or_else(|| self.entries.get(legacy_resource_pack_block_alias(name)?));
        let Some(entry) = entry else {
            return Ok([0; BlockFace::ALL.len()]);
        };
        let exponent = match entry.extra.get("ambient_occlusion_exponent") {
            None | Some(Value::Null) => 0,
            Some(value) => encode_exponent(value).ok_or_else(|| AssetError::InvalidCompiledAssets {
                detail: format!(
                    "unsupported world leaf ambient_occlusion_exponent for {}: carrier requires a positive exact hundredth within its bounded field",
                    record.name
                )
                .into(),
            })?,
        };
        Ok(self
            .isotropic_face_flags(record)
            .map(|flags| exponent | flags))
    }
}

fn encode_exponent(value: &Value) -> Option<u32> {
    // Current native block-graphics parse stores the f32 directly.
    // Admit only values our compact carrier represents exactly; do not round
    // custom exponents or alias authored zero to the omitted default of one.
    let value = value.as_f64()? as f32;
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    let encoded = (value * MATERIAL_LEAF_AO_EXPONENT_SCALE as f32).round() as u32;
    if encoded == 0
        || encoded > MATERIAL_LEAF_AO_EXPONENT_MAX
        || material_leaf_ao_exponent(encoded << MATERIAL_LEAF_AO_EXPONENT_SHIFT).to_bits()
            != value.to_bits()
    {
        return None;
    }
    Some(encoded << MATERIAL_LEAF_AO_EXPONENT_SHIFT)
}

fn is_isotropic(value: Option<&Value>, face: BlockFace) -> bool {
    // Vanilla scalar bool selects all/none. A non-null side
    // chooses the up/down/side map, otherwise the six explicit face names.
    // Unknown and non-boolean entries do not set bits; caller ignores the
    // helper's failure return, so preserve its admitted mask on odd objects.
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::Object(object)) => {
            let key = if face.is_horizontal()
                && object.get("side").is_some_and(|value| !value.is_null())
            {
                "side"
            } else {
                match face {
                    BlockFace::West => "west",
                    BlockFace::East => "east",
                    BlockFace::Down => "down",
                    BlockFace::Up => "up",
                    BlockFace::North => "north",
                    BlockFace::South => "south",
                }
            };
            object.get(key).and_then(Value::as_bool).unwrap_or(false)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_isotropic_mask_handles_scalar_side_and_odd_objects() {
        for (value, expected) in [
            (serde_json::json!(true), [true; 6]),
            (serde_json::json!(false), [false; 6]),
            (
                serde_json::json!({"up": true, "down": true}),
                [false, false, true, true, false, false],
            ),
            (
                serde_json::json!({"side": false, "west": true, "up": true}),
                [false, false, false, true, false, false],
            ),
            (
                serde_json::json!({"side": true, "east": false, "down": false}),
                [true, true, false, false, true, true],
            ),
            (
                serde_json::json!({"side": null, "north": true, "west": 1, "all": true}),
                [false, false, false, false, true, false],
            ),
            (
                serde_json::json!({"side": 1, "east": true, "up": true}),
                [false, false, false, true, false, false],
            ),
        ] {
            assert_eq!(
                BlockFace::ALL.map(|face| is_isotropic(Some(&value), face)),
                expected
            );
        }
    }

    #[test]
    fn exponent_encoding_is_exact_bounded_and_does_not_alias_authored_zero() {
        for value in [0.01, 0.8, 1.0, 2.55] {
            let flags = encode_exponent(&serde_json::json!(value)).unwrap();
            assert_eq!(
                assets::material_leaf_ao_exponent(flags).to_bits(),
                (value as f32).to_bits()
            );
        }
        assert_eq!(assets::material_leaf_ao_exponent(0), 1.0);
        for value in [
            serde_json::json!(0),
            serde_json::json!(-0.8),
            serde_json::json!(0.805),
            serde_json::json!(2.56),
            serde_json::json!("0.8"),
        ] {
            assert_eq!(encode_exponent(&value), None, "{value}");
        }
    }
}

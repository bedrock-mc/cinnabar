//! Immutable animation and model tables packed into one vertex storage binding.

/// Packs word offsets relative to the payload after the four-word header.
/// Empty animation tables retain valid diagnostic records; model length stays independent.
pub(super) fn encode_asset_table_words(
    animations: &[[u32; 4]],
    frames: &[u32],
    models: &[u32],
) -> Option<Vec<u32>> {
    let animation_count = u32::try_from(animations.len().max(1)).ok()?;
    let frame_offset = animation_count.checked_mul(4)?;
    let model_offset = frame_offset.checked_add(u32::try_from(frames.len().max(1)).ok()?)?;
    let model_word_count = u32::try_from(models.len()).ok()?;
    let payload_len = model_offset.checked_add(model_word_count)?;
    let capacity = usize::try_from(payload_len).ok()?.checked_add(4)?;
    let mut words = Vec::with_capacity(capacity);
    words.extend([
        animation_count,
        frame_offset,
        model_offset,
        model_word_count,
    ]);
    if animations.is_empty() {
        words.extend([0, 1, 1, 0]);
    } else {
        words.extend(animations.iter().flatten().copied());
    }
    if frames.is_empty() {
        words.push(assets::TextureRef::DIAGNOSTIC.raw());
    } else {
        words.extend_from_slice(frames);
    }
    words.extend_from_slice(models);
    Some(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_preserve_animation_records_frames_and_model_bounds() {
        let animations = [[1, 2, 3, 1], [0, 1, 4, 0]];
        let frames = [11, 23, 37];
        let models = [1, 0, 1, 0, 99, 101];
        let words = encode_asset_table_words(&animations, &frames, &models).unwrap();
        let payload = &words[4..];
        let [count, frame_offset, model_offset, model_len]: [u32; 4] =
            words[..4].try_into().unwrap();
        assert_eq!(count, 2);
        assert_eq!(&payload[..frame_offset as usize], animations.as_flattened());
        assert_eq!(
            &payload[frame_offset as usize..model_offset as usize],
            frames
        );
        assert_eq!(
            &payload[model_offset as usize..][..model_len as usize],
            models
        );
        assert_eq!(payload.len(), (model_offset + model_len) as usize);
    }

    #[test]
    fn empty_animation_tables_keep_diagnostic_sentinels_outside_model_region() {
        let models = [0];
        let words = encode_asset_table_words(&[], &[], &models).unwrap();
        assert_eq!(words[0], 1);
        assert_eq!(&words[4..8], [0, 1, 1, 0]);
        assert_eq!(words[8], assets::TextureRef::DIAGNOSTIC.raw());
        let model_offset = 4 + words[2] as usize;
        assert_eq!(words[3], models.len() as u32);
        assert_eq!(&words[model_offset..], models);
    }
}

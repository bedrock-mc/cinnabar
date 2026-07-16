#[cfg(test)]
pub(in crate::chunk) fn absolutize_model_draw_refs(
    draw_refs: &mut [[u32; 2]],
    model_word_start: u32,
) -> Option<()> {
    if !model_word_start.is_multiple_of(4) {
        return None;
    }
    let model_record_base = model_word_start / 4;
    if draw_refs
        .iter()
        .any(|words| words[0].checked_add(model_record_base).is_none())
    {
        return None;
    }
    for words in draw_refs {
        words[0] += model_record_base;
    }
    Some(())
}

pub(in crate::chunk) fn absolutize_partitioned_model_draw_refs(
    opaque_draw_refs: &mut [[u32; 2]],
    blend_draw_refs: &mut [[u32; 2]],
    model_word_start: u32,
) -> Option<()> {
    if !model_word_start.is_multiple_of(4) {
        return None;
    }
    let model_record_base = model_word_start / 4;
    if opaque_draw_refs
        .iter()
        .chain(blend_draw_refs.iter())
        .any(|words| words[0].checked_add(model_record_base).is_none())
    {
        return None;
    }
    for words in opaque_draw_refs
        .iter_mut()
        .chain(blend_draw_refs.iter_mut())
    {
        words[0] += model_record_base;
    }
    Some(())
}

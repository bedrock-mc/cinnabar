//! Optional item references supplied by our JSON-UI screen controllers.

use std::collections::BTreeMap;

use serde_json::Value;

use ui::IconRef;

pub(super) fn icon<'a>(
    data: &BTreeMap<String, Value>,
    icons: &'a [IconRef],
    id_aux: &'a [(i64, IconRef)],
) -> Option<&'a IconRef> {
    match data.get("#item_renderer_data") {
        // A present Null is our explicit Option::None, not an unanswered
        // binding. It must neither reuse an old index nor fall back to id_aux.
        Some(value) => icons.get(index(value.as_f64()?, icons.len())?),
        None => {
            let key = data.get("#item_id_aux")?.as_f64()? as i64;
            Some(&id_aux.iter().find(|(id, _)| *id == key)?.1)
        }
    }
}

fn index(value: f64, len: usize) -> Option<usize> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value >= len as f64 {
        return None;
    }
    Some(value as usize)
}

#[cfg(test)]
mod tests;

//! Item icons for `inventory_item_renderer` controls, supplied by each screen's controller.

use std::collections::BTreeMap;

use serde_json::Value;

use ui::IconRef;

use super::{
    Canvas,
    screen::{CustomDraw, CustomRenderer, PaintError},
};

/// Draws `inventory_item_renderer` controls from a screen's item icons.
#[derive(Default)]
pub struct ItemRenderer<'a> {
    /// Icons `#item_renderer_data` indexes.
    pub icons: &'a [IconRef],
    /// Icons an `#item_id_aux` names, by that value.
    pub id_aux: &'a [(i64, IconRef)],
}

impl CustomRenderer for ItemRenderer<'_> {
    fn paint(
        &mut self,
        node: &CustomDraw<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<bool, PaintError> {
        if node.renderer != "inventory_item_renderer" {
            return Ok(false);
        }
        if let Some(icon) = icon(node.data, self.icons, self.id_aux) {
            canvas.push(icon.visual(node.alpha([255; 4])), node.dest)?;
        }
        Ok(true)
    }
}

fn icon<'a>(
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
    .filter(|icon| icon.uv[0] < icon.uv[2] && icon.uv[1] < icon.uv[3])
}

fn index(value: f64, len: usize) -> Option<usize> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value >= len as f64 {
        return None;
    }
    Some(value as usize)
}

#[cfg(test)]
mod tests;

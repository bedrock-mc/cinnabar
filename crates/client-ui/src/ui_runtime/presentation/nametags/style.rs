//! Cosmetic transforms preserve whole-label admission and draw order.

use super::*;

/// Counts the final records emitted for one plate or textured run.
pub(super) fn record_cost(style: Option<&ui::mod_hud::NametagStyle>, plate: bool) -> usize {
    1 + style.map_or(0, |style| {
        if plate {
            4 * usize::from(style.border_color.is_some())
        } else {
            usize::from(style.shadow)
        }
    })
}

/// Applies bounded cosmetic overrides without changing record order or depth selection.
pub(in crate::ui_runtime::presentation) fn style_scene(
    scene: &mut NametagScene,
    style: &ui::mod_hud::NametagStyle,
    own_actor: Option<u64>,
) {
    if style.hide {
        scene.records.clear();
        scene.labels.clear();
        return;
    }
    let original = std::mem::take(&mut scene.records);
    let mut labels = std::mem::take(&mut scene.labels);
    if labels.is_empty() && !original.is_empty() {
        labels.push(render_model::NametagLabel {
            records: 0..original.len(),
            view_depth: 0.0,
        });
    }
    let mut nearest: Vec<_> = (0..labels.len()).collect();
    nearest.sort_by(|&a, &b| labels[a].view_depth.total_cmp(&labels[b].view_depth));
    let mut admitted = vec![false; labels.len()];
    let mut available = MAX_NAMETAG_RECORDS;
    for index in nearest {
        let cost: usize = original[labels[index].records.clone()]
            .iter()
            .map(|record| record_cost(Some(style), record.uv[2] < 0.0))
            .sum();
        if cost <= available {
            admitted[index] = true;
            available -= cost;
        }
    }
    for (label, admitted) in labels.into_iter().zip(admitted) {
        if !admitted {
            continue;
        }
        let first = scene.records.len();
        for mut record in original[label.records].iter().copied() {
            for coordinate in &mut record.rect {
                *coordinate *= style.scale;
            }
            record.line_lift *= style.scale;
            if record.uv[2] < 0.0 {
                if let Some(color) = style.background_color {
                    record.color = color;
                }
                record.color[3] *= style.opacity;
                if let Some(mut color) = style.border_color {
                    color[3] *= style.opacity;
                    let [left, top, right, bottom] = record.rect;
                    let width = style.border_width * style.scale;
                    for rect in [
                        [left - width, top - width, right + width, top],
                        [left - width, bottom, right + width, bottom + width],
                        [left - width, top, left, bottom],
                        [right, top, right + width, bottom],
                    ] {
                        scene.records.push(NametagRecord {
                            rect,
                            color,
                            ..record
                        });
                    }
                }
            } else {
                let own = own_actor == Some(record.runtime_id);
                let text_color = if own {
                    style.own_text_color.or(style.text_color)
                } else {
                    style.text_color
                };
                let replace = if own && style.own_text_color.is_some() {
                    style.replace_own_text_color
                } else {
                    style.replace_text_color
                };
                if let Some(color) = text_color {
                    for (channel, tint) in record.color[..3].iter_mut().zip(color) {
                        *channel = if replace { tint } else { *channel * tint };
                    }
                    record.color[3] *= color[3];
                }
                record.color[3] *= style.opacity;
                if style.shadow {
                    let mut shadow = record;
                    shadow.rect = [
                        record.rect[0] + style.scale,
                        record.rect[1] + style.scale,
                        record.rect[2] + style.scale,
                        record.rect[3] + style.scale,
                    ];
                    shadow.color = [0., 0., 0., record.color[3] * 0.8];
                    scene.records.push(shadow);
                }
            }
            scene.records.push(record);
        }
        scene.labels.push(render_model::NametagLabel {
            records: first..scene.records.len(),
            view_depth: label.view_depth,
        });
    }
}

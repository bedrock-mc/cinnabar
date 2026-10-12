//! Camera-projected, bounded cosmetic labels rendered entirely through JSON-UI.
use super::template::{named, rounded};
use json_ui::{DataSource, Scalar};
use serde_json::{Value, json};
use ui::mod_hud::{Hud, WorldLabel};
/// Ignores live positions and equal-sized text updates when retaining the catalog.
pub(super) fn same_shape(a: &[WorldLabel], b: &[WorldLabel]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.icon == b.icon
                && a.icon_scale == b.icon_scale
                && a.scale == b.scale
                && a.padding == b.padding
                && a.border_width == b.border_width
                && a.background_opacity == b.background_opacity
                && a.color == b.color
                && a.shadow == b.shadow
                && a.text
                    .lines()
                    .map(|s| s.chars().count())
                    .eq(b.text.lines().map(|s| s.chars().count()))
        })
}
/// Retained JSON-UI geometry for a world label; position is supplied as a live binding.
pub(super) fn control(index: usize, label: &ui::mod_hud::WorldLabel) -> Value {
    let scale = f64::from(label.scale);
    let padding = f64::from(label.padding) * scale;
    let width = (label
        .text
        .lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0) as f64
        * 6.0
        * scale
        + padding * 2.0)
        .max(1.0);
    let icon_height = if label.icon.is_some() {
        10.0 * f64::from(label.icon_scale)
    } else {
        0.
    };
    let height =
        label.text.lines().count().max(1) as f64 * 10.0 * scale + padding * 2.0 + icon_height;
    let mut controls = vec![named(
        "background",
        rounded(
            [width, height],
            [0., 0.],
            0.,
            [0., 0., 0., label.background_opacity],
        ),
    )];
    let border = f64::from(label.border_width) * scale;
    if border > 0.0 {
        for (n, (size, offset)) in [
            ([width, border], [0., 0.]),
            ([width, border], [0., height - border]),
            ([border, height], [0., 0.]),
            ([border, height], [width - border, 0.]),
        ]
        .into_iter()
        .enumerate()
        {
            controls.push(named(
                &format!("edge_{n}"),
                rounded(size, offset, 0., label.color),
            ));
        }
    }
    if let Some(icon) = &label.icon {
        controls.push(named("icon",json!({"type":"label","size":[width,icon_height],"anchor_from":"top_left","anchor_to":"top_left","offset":[0.,padding],"text":icon,"localize":false,"shadow":label.shadow,"font_scale_factor":0.8*f64::from(label.icon_scale),"color":label.color,"text_alignment":"center"})));
    }
    controls.push(named("text",json!({"type":"label","size":[width-2.*padding,height-2.*padding],"offset":[padding,padding+icon_height],"anchor_from":"top_left","anchor_to":"top_left","text":format!("#world_label_{index}_text"),"bindings":[{"binding_name":format!("#world_label_{index}_text")}],"localize":false,"shadow":label.shadow,"font_scale_factor":0.8*scale,"color":label.color,"text_alignment":"center"})));
    named(
        &format!("world_label_{index}"),
        json!({"type":"panel","size":[width,height],"anchor_from":"top_left","anchor_to":"top_left","bindings":[{"binding_name":format!("#world_label_{index}_offset"),"binding_name_override":"#offset"},{"binding_name":format!("#world_label_{index}_visible"),"binding_name_override":"#visible"}],"controls":controls}),
    )
}
/// Projects finite anchors through the current camera, excluding the near plane and rear hemisphere.
pub(super) fn data(
    data: &mut DataSource,
    hud: &Hud,
    viewport: [f64; 2],
    projection: Option<[f32; 16]>,
) {
    let matrix = projection.map(|p| glam::Mat4::from_cols_array(&p));
    for (index, label) in hud.world_labels.iter().enumerate() {
        let offset = project(label, matrix, viewport);
        let visible = offset.is_some();
        let offset = offset.unwrap_or([0., 0.]);
        data.set_global(
            format!("#world_label_{index}_text"),
            Scalar::Text(label.text.clone()),
        );
        data.set_global(
            format!("#world_label_{index}_offset"),
            Scalar::Json(json!(offset)),
        );
        data.set_global(
            format!("#world_label_{index}_visible"),
            Scalar::Bool(visible),
        );
    }
}

/// Screen-space center placement for one validated world anchor.
fn project(label: &WorldLabel, matrix: Option<glam::Mat4>, viewport: [f64; 2]) -> Option<[f64; 2]> {
    let point = matrix.map(|m| m * glam::Vec3::from_array(label.position).extend(1.0));
    let visible = point.is_some_and(|p| {
        p.is_finite()
            && p.w > 0.001
            && p.z >= 0.0
            && p.z <= p.w
            && p.x.abs() <= p.w
            && p.y.abs() <= p.w
            && label.near_angle.is_none_or(|angle| {
                let m = matrix.unwrap();
                let x = p.x / m.row(0).truncate().length();
                let y = p.y / m.row(1).truncate().length();
                (x.hypot(y) / p.w).atan().to_degrees() <= angle
            })
    });
    let offset = if visible {
        let p = point.unwrap();
        let scale = f64::from(label.scale);
        let width = (label
            .text
            .lines()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0) as f64
            * 6.0
            + f64::from(label.padding) * 2.0)
            * scale;
        let height = (label.text.lines().count().max(1) as f64 * 10.0
            + f64::from(label.padding) * 2.0)
            * scale
            + if label.icon.is_some() {
                10.0 * f64::from(label.icon_scale)
            } else {
                0.
            };
        [
            (f64::from(p.x / p.w) * 0.5 + 0.5) * viewport[0] - width * 0.5,
            (0.5 - f64::from(p.y / p.w) * 0.5) * viewport[1] - height * 0.5,
        ]
    } else {
        return None;
    };

    Some(offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Observes the published visibility binding rather than repeating projection math.
    fn visible(position: [f32; 3], projection: Option<[f32; 16]>) -> bool {
        let mut hud = Hud::default();
        hud.world_labels.push(WorldLabel {
            position,
            text: "Marker".into(),
            ..Default::default()
        });
        let matrix = projection.map(|p| glam::Mat4::from_cols_array(&p));
        project(&hud.world_labels[0], matrix, [640., 360.]).is_some()
    }
    #[test]
    fn labels_hide_without_camera_behind_it_and_outside_frustum() {
        let matrix =
            glam::camera::rh::proj::directx::perspective(1.0, 16. / 9., 0.1, 1000.).to_cols_array();
        assert!(!visible([0., 0., -10.], None));
        assert!(visible([0., 0., -10.], Some(matrix)));
        assert!(!visible([0., 0., 10.], Some(matrix)));
        assert!(!visible([100., 0., -10.], Some(matrix)));
    }
    #[test]
    fn live_equal_width_content_and_positions_retain_geometry() {
        let a = WorldLabel {
            text: "Home 12m".into(),
            ..Default::default()
        };
        let b = WorldLabel {
            text: "Home 13m".into(),
            position: [10., 20., 30.],
            ..a.clone()
        };
        assert!(same_shape(&[a], &[b]));
    }
}

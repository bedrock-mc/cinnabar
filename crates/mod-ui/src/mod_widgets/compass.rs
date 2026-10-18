//! Retained compass geometry receives current native poses through ordinary scalar bindings.

use json_ui::{DataSource, Scalar};
use serde_json::{Value, json};
use ui::mod_hud::{
    COMPASS_DIRECTIONS, COMPASS_HEIGHT, COMPASS_TICKS, Card, MAX_COMPASS_MARKERS, compass_bearing,
    compass_offset,
};

const DIRECTION_LABEL_WIDTH: f64 = 30.;

/// Binds one retained node property without embedding any observed pose in its geometry.
fn bind(mut node: Value, key: &str, target: &str) -> Value {
    node["bindings"] = json!([{"binding_name":key,"binding_name_override":target}]);
    node
}

/// Creates a fixed plain-text label centered on its authored anchor.
fn label(text: &str, width: f64, offset: [f64; 2], card: &Card, color: &str) -> Value {
    let k = f64::from(card.scale);
    json!({"type":"label","text":text,"size":[width,"default"],"max_size":[width,14.*k],
        "offset":offset,"anchor_from":"top_left","anchor_to":"left_middle",
        "text_alignment":"center","localize":false,"shadow":card.shadow,
        "font_scale_factor":0.8*k*f64::from(card.text_scale),"clip_children":true,
        "bindings":[{"binding_name":color,"binding_name_override":"#color"}]})
}

/// Builds fixed tick and marker slots once; all movement stays in native data bindings.
pub(super) fn control(card: &Card, index: usize, header: f64) -> Value {
    let k = f64::from(card.scale);
    let width = f64::from(card.width) * k;
    let prefix = format!("#compass_{index}");
    let mut children = Vec::new();
    let major_spacing = COMPASS_TICKS / COMPASS_DIRECTIONS.len();
    for tick in 0..COMPASS_TICKS {
        let mut parts = vec![super::template::named(
            "line",
            bind(
                super::template::rounded(
                    [
                        k,
                        if tick.is_multiple_of(major_spacing) {
                            5. * k
                        } else {
                            3. * k
                        },
                    ],
                    [0., 0.],
                    0.,
                    [1.; 4],
                ),
                &format!("{prefix}_color"),
                "#color",
            ),
        )];
        if tick.is_multiple_of(major_spacing) {
            parts.push(super::template::named(
                "direction",
                label(
                    COMPASS_DIRECTIONS[tick / major_spacing],
                    DIRECTION_LABEL_WIDTH * k,
                    [-DIRECTION_LABEL_WIDTH * 0.5 * k, 12. * k],
                    card,
                    &format!("{prefix}_color"),
                ),
            ));
        }
        children.push(super::template::named(&format!("tick_{tick}"),json!({
            "type":"panel","size":[k,20.*k],"anchor_from":"top_left","anchor_to":"top_left","propagate_alpha":true,
            "controls":parts,"bindings":[
                {"binding_name":format!("{prefix}_tick_{tick}_offset"),"binding_name_override":"#offset"},
                {"binding_name":format!("{prefix}_tick_{tick}_alpha"),"binding_name_override":"#alpha"},
                {"binding_name":format!("{prefix}_tick_{tick}_visible"),"binding_name_override":"#visible"}]})));
    }
    let needle = (0..3)
        .map(|row| {
            super::template::named(
                &format!("line_{row}"),
                bind(
                    super::template::rounded(
                        [(7. - row as f64 * 2.) * k, k],
                        [(row as f64) * k, (row as f64) * k],
                        0.,
                        [1.; 4],
                    ),
                    &format!("{prefix}_marker_color"),
                    "#color",
                ),
            )
        })
        .collect::<Vec<_>>();
    children.push(super::template::named("needle",bind(json!({"type":"panel","size":[7.*k,3.*k],
        "offset":[(width-7.*k)*0.5,0.],"anchor_from":"top_left","anchor_to":"top_left","controls":needle}),
        &format!("{prefix}_needle_visible"),"#visible")));
    let mut heading = label(
        "",
        48. * k,
        [(width - 48. * k) * 0.5, 25. * k],
        card,
        &format!("{prefix}_marker_color"),
    );
    heading["text"] = json!(format!("{prefix}_heading"));
    heading["bindings"].as_array_mut().unwrap().extend([
        json!({"binding_name":format!("{prefix}_heading")}),
        json!({"binding_name":format!("{prefix}_value_visible"),"binding_name_override":"#visible"}),
    ]);
    children.push(super::template::named("heading", heading));
    for marker in 0..MAX_COMPASS_MARKERS {
        let mut dot = super::template::rounded([4. * k, 4. * k], [0., 0.], 2. * k, [1.; 4]);
        dot["bindings"] = json!([
            {"binding_name":format!("{prefix}_waypoint_{marker}_offset"),"binding_name_override":"#offset"},
            {"binding_name":format!("{prefix}_waypoint_{marker}_visible"),"binding_name_override":"#visible"},
            {"binding_name":format!("{prefix}_waypoint_{marker}_color"),"binding_name_override":"#color"},
        ]);
        children.push(super::template::named(&format!("waypoint_{marker}"), dot));
    }
    json!({"type":"panel","size":[width,f64::from(COMPASS_HEIGHT)*k],"offset":[0.,header*k],
        "anchor_from":"top_left","anchor_to":"top_left","clip_children":true,"controls":children})
}

/// Projects the latest native pose into fixed slots without replacing their template or icons.
pub(super) fn data(data: &mut DataSource, card: &Card, index: usize) {
    let Some(compass) = &card.compass else { return };
    let prefix = format!("#compass_{index}");
    let k = f64::from(card.scale);
    data.set_global(
        format!("{prefix}_color"),
        Scalar::Json(json!(compass.color)),
    );
    data.set_global(
        format!("{prefix}_marker_color"),
        Scalar::Json(json!(compass.marker_color)),
    );
    data.set_global(
        format!("{prefix}_needle_visible"),
        Scalar::Bool(compass.show_marker && compass.frame.is_some()),
    );
    data.set_global(
        format!("{prefix}_value_visible"),
        Scalar::Bool(compass.show_value),
    );
    data.set_global(
        format!("{prefix}_heading"),
        Scalar::Text(compass.frame.as_ref().map_or_else(
            || "--".into(),
            |pose| format!("{:.0}°", pose.heading.round().rem_euclid(360.)),
        )),
    );
    for tick in 0..COMPASS_TICKS {
        let x = compass.frame.as_ref().and_then(|pose| {
            compass_offset(
                tick as f32 * 360. / COMPASS_TICKS as f32,
                pose.heading,
                card.width,
            )
        });
        data.set_global(
            format!("{prefix}_tick_{tick}_visible"),
            Scalar::Bool(x.is_some()),
        );
        let fade = x.map_or(0., |x| {
            (f64::from(x.min(card.width - x)) / (DIRECTION_LABEL_WIDTH * 0.5)).clamp(0., 1.)
        });
        data.set_global(format!("{prefix}_tick_{tick}_alpha"), Scalar::Num(fade));
        data.set_global(
            format!("{prefix}_tick_{tick}_offset"),
            Scalar::Json(json!([f64::from(x.unwrap_or(0.)) * k, 3. * k])),
        );
    }
    for marker in 0..MAX_COMPASS_MARKERS {
        let waypoint = compass.waypoints.get(marker);
        let x = compass
            .frame
            .as_ref()
            .zip(waypoint)
            .and_then(|(pose, marker)| {
                compass_bearing(pose.position, marker.position)
                    .and_then(|bearing| compass_offset(bearing, pose.heading, card.width))
            });
        data.set_global(
            format!("{prefix}_waypoint_{marker}_visible"),
            Scalar::Bool(x.is_some()),
        );
        data.set_global(
            format!("{prefix}_waypoint_{marker}_offset"),
            Scalar::Json(json!([(f64::from(x.unwrap_or(0.)) - 2.) * k, 19. * k])),
        );
        data.set_global(
            format!("{prefix}_waypoint_{marker}_color"),
            Scalar::Json(json!(waypoint.map_or([1.; 4], |w| w.color))),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ModUi;
    use json_ui::{Context, Draw, LayoutEnv, TextMeasure, TextureMeta, TextureSource, ViewState};
    use std::sync::Arc;
    use ui::mod_hud::{COMPASS_DEFAULT_WIDTH, Compass, CompassFrame, Hud};
    use ui_engine::cached_screen::CachedScreen;

    struct Metrics;
    impl TextMeasure for Metrics {
        /// Keeps layout deterministic without requiring a local font carrier.
        fn extent(&self, text: &str) -> [f64; 2] {
            [text.chars().count() as f64 * 4., 8.]
        }
    }
    impl TextureSource for Metrics {
        /// The compass uses native fills and plain text rather than external textures.
        fn texture(&self, _: &str) -> Option<TextureMeta> {
            None
        }
    }

    /// Native pose changes move actual rendered labels while retaining one admitted catalog.
    #[test]
    fn compass_motion_reuses_geometry_and_updates_rendered_positions() {
        let mut hud = Hud {
            cards: vec![Card {
                id: "direction".into(),
                width: COMPASS_DEFAULT_WIDTH,
                compass: Some(Compass {
                    frame: Some(CompassFrame {
                        heading: 0.,
                        position: [0.; 3],
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut ui = ModUi::default();
        ui.set_mod_hud(Some(&hud)).unwrap();
        let original = Arc::clone(&ui.mod_widgets.as_ref().unwrap().catalog);
        let mut screen = CachedScreen::default();
        let mut south_x = Vec::new();
        let env = LayoutEnv {
            text: &Metrics,
            textures: &Metrics,
        };
        for heading in [0., 1., 359.] {
            hud.cards[0]
                .compass
                .as_mut()
                .unwrap()
                .frame
                .as_mut()
                .unwrap()
                .heading = heading;
            ui.set_mod_hud(Some(&hud)).unwrap();
            let widgets = ui.mod_widgets.as_ref().unwrap();
            assert!(Arc::ptr_eq(&original, &widgets.catalog));
            let render = screen
                .render_shared_with(
                    "cinnabar_personal_hud.cards",
                    &widgets.catalog,
                    &Context::default(),
                    Arc::clone(&widgets.data),
                    ([800., 500.], 1., [0; 3]),
                    &env,
                    &ViewState::default(),
                )
                .unwrap();
            let south = render
                .nodes
                .iter()
                .find(|node| matches!(&node.draw,Draw::Text {text,..} if text=="S"))
                .unwrap();
            south_x.push(south.dest.x);
            assert!(render.nodes.len() < COMPASS_TICKS * 4 + MAX_COMPASS_MARKERS + 8);
            assert!(
                render
                    .nodes
                    .iter()
                    .filter(|node| matches!(node.draw, Draw::Text { .. }))
                    .all(|node| node.clip.x >= 0.
                        && node.clip.x + node.clip.w <= f64::from(COMPASS_DEFAULT_WIDTH))
            );
            for node in render.nodes.iter().filter(|node| matches!(&node.draw,Draw::Text {text,..} if COMPASS_DIRECTIONS.contains(&text.as_str()))) {
                if node.dest.x < node.clip.x || node.dest.x+node.dest.w > node.clip.x+node.clip.w {
                    assert!(node.alpha < 1.,"clipped edge directions must fade rather than end abruptly");
                }
            }
        }
        let step = f64::from(COMPASS_DEFAULT_WIDTH) / 180.;
        assert!((south_x[0] - south_x[1] - step).abs() < 0.01);
        assert!((south_x[2] - south_x[0] - step).abs() < 0.01);
        hud.cards[0].compass.as_mut().unwrap().frame = None;
        ui.set_mod_hud(Some(&hud)).unwrap();
        let widgets = ui.mod_widgets.as_ref().unwrap();
        let render = screen
            .render_shared_with(
                "cinnabar_personal_hud.cards",
                &widgets.catalog,
                &Context::default(),
                Arc::clone(&widgets.data),
                ([800., 500.], 1., [0; 3]),
                &env,
                &ViewState::default(),
            )
            .unwrap();
        assert!(!render.nodes.iter().any(|node|matches!(&node.draw,Draw::Text {text,..} if COMPASS_DIRECTIONS.contains(&text.as_str()))));
        assert!(
            render
                .nodes
                .iter()
                .any(|node| matches!(&node.draw,Draw::Text {text,..} if text=="--"))
        );
    }
}

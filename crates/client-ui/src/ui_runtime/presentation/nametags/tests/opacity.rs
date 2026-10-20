use super::*;

/// Replacement removes a formatted tint, while opacity still preserves native sneaking alpha.
#[test]
fn replacement_and_default_text_opacity_preserve_native_coverage() {
    let glyph = NametagRecord {
        anchor: [1., 2., 3.],
        color: [0., 1., 0., 0.125],
        uv: [0., 0., 1., 1.],
        ..Default::default()
    };
    let remote = NametagRecord {
        anchor: [4., 5., 6.],
        ..glyph
    };
    let mut scene = NametagScene {
        records: vec![glyph, remote],
        ..Default::default()
    };
    let style: ui::mod_hud::NametagStyle = serde_json::from_value(serde_json::json!({"opacity":0.5,"own_text_color":[1.,0.,0.,1.],"replace_own_text_color":true})).unwrap();
    style_scene(&mut scene, &style, Some(glyph.anchor));
    assert_eq!(scene.records[0].color, [1., 0., 0., 0.0625]);
    assert_eq!(scene.records[1].color, [0., 1., 0., 0.0625]);
    assert_eq!(
        scene.records[0].fragment_color([1.; 4]),
        Some([1., 0., 0., 0.0625])
    );
    assert_eq!(
        scene.records[1].fragment_color([1.; 4]),
        Some([0., 1., 0., 0.0625])
    );
    assert!(
        scene.records[0]
            .fragment_color([1., 1., 1., 0.05])
            .is_none()
    );
}

/// Zero opacity discards glyphs and plates; visible glyphs retain atlas coverage and native alpha.
#[test]
fn faded_sneak_and_through_wall_glyphs_keep_their_atlas_coverage() {
    for sneaking in [false, true] {
        for opacity in [0.0, 0.5, 1.0] {
            let mut label = anchor("A");
            label.depth_tested = sneaking;
            label.text_alpha = if sneaking { DIM_TEXT_ALPHA } else { 1.0 };
            let mut scene = scene(&[label]);
            let style = serde_json::from_value(serde_json::json!({
                "opacity": opacity,
                "text_color": [1.0, 1.0, 1.0, 0.25],
            }))
            .unwrap();
            style_scene(&mut scene, &style, None);
            for record in scene.records.iter().filter(|record| record.text != 0) {
                let native_alpha = if sneaking { DIM_TEXT_ALPHA } else { 1.0 };
                let fragment = record.fragment_color([1.0, 1.0, 1.0, 0.5]);
                if opacity == 0.0 {
                    assert!(fragment.is_none());
                } else {
                    assert!(
                        (fragment.unwrap()[3] - native_alpha * opacity * 0.25 * 0.5).abs() < 1e-6
                    );
                }
                assert!(
                    record
                        .fragment_color([1.0, 1.0, 1.0, render_model::NAMETAG_ALPHA_CUTOFF])
                        .is_none()
                );
            }
            // Visible plates ignore atlas contents; zero opacity discards them.
            assert_eq!(
                scene.records[0].fragment_color([0.0; 4]).is_some(),
                opacity > 0.0
            );
        }
    }
}

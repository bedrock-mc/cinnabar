//! Counter geometry remains attached to an eligible name and within the native record budget.

use super::*;

/// Finds the full depth pass for one independently colored native glyph run.
fn colored_record(scene: &NametagScene, rgb: [f32; 3]) -> &NametagRecord {
    scene
        .records
        .iter()
        .find(|record| {
            record.text != 0
                && record.depth_tested == 1
                && record.color[..3]
                    .iter()
                    .zip(rgb)
                    .all(|(a, b)| (*a - b).abs() < 0.002)
        })
        .unwrap()
}

/// Actual glyph cells determine inline order, separate lines, color alpha, and the expanded backing plate.
#[test]
fn totem_annotation_preserves_server_lines_tints_and_native_depth_passes() {
    for separate in [false, true] {
        for before in [false, true] {
            let mut tag = anchor("Player\n7 wins");
            let policy = ui::mod_hud::TotemCounterStyle {
                show_prefix: true,
                render_first: before,
                separate_nametag: separate,
                prefix_color: [0.2, 0.4, 0.6, 0.5],
                ..Default::default()
            };
            tag.set_counter_annotation(policy.annotation(2).map(Arc::new));
            let painted = scene(&[tag]);
            let prefix = colored_record(&painted, [0.2, 0.4, 0.6]);
            assert!((prefix.color[3] - 128. / 255.).abs() < 0.002);
            let count = colored_record(&painted, [85. / 255., 1., 85. / 255.]);
            assert!(prefix.rect[2] <= count.rect[0] + 1.);
            let names: Vec<_> = painted
                .records
                .iter()
                .filter(|record| {
                    record.text != 0 && record.depth_tested == 1 && record.color == [1.; 4]
                })
                .collect();
            assert_eq!(
                names.len(),
                2,
                "server name and score remain separate native lines"
            );
            if separate {
                assert!(if before {
                    count.rect[1] < names[0].rect[1]
                } else {
                    count.rect[1] > names[1].rect[1]
                });
            } else {
                assert!(if before {
                    count.rect[0] < names[0].rect[0]
                } else {
                    count.rect[0] > names[0].rect[0]
                });
            }
            assert_eq!(
                painted
                    .records
                    .iter()
                    .filter(|record| record.uv[2] < 0.)
                    .count(),
                2 + usize::from(separate)
            );
        }
    }
}

/// Crowded annotated labels use the existing ceiling rather than creating a second unbounded render stream.
#[test]
fn totem_annotation_crowds_respect_nametag_record_limits() {
    let policy = ui::mod_hud::TotemCounterStyle {
        show_prefix: true,
        separate_nametag: true,
        ..Default::default()
    };
    let tags: Vec<_> = (0..MAX_PRESENTED_NAMETAGS)
        .map(|n| {
            let mut tag = anchor("Player\n7 wins\nTeam");
            tag.runtime_id = n as u64 + 1;
            tag.set_counter_annotation(policy.annotation(2).map(Arc::new));
            tag
        })
        .collect();
    let painted = scene(&tags);
    assert!(!painted.records.is_empty());
    assert!(painted.records.len() <= MAX_NAMETAG_RECORDS);
}

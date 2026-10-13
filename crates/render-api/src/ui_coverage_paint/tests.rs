use super::*;

/// Every pair of channel bytes survives the float carrier and its shader-compatible rounding.
#[test]
fn paint_color_bytes_round_trip_without_loss() {
    for bytes in 0..=u16::MAX {
        let paint = UiCoveragePaint {
            fill: [[17, 31, 43, 255], [bytes as u8; 4]],
            border: [(bytes >> 8) as u8; 4],
        };
        let (tint, overlay) = paint.vertex_colors();
        assert_eq!(UiCoveragePaint::from_vertex_colors(tint, overlay), paint);
    }
}

/// Coverage multiplies the authored straight gradient only after its position is evaluated.
#[test]
fn gradient_and_border_contributions_remain_independent() {
    let paint = UiCoveragePaint {
        fill: [[255, 0, 0, 255], [0, 0, 255, 0]],
        border: [0, 255, 0, 128],
    };
    assert_eq!(paint.sample(0.75, [1.0, 0.0]), [0.0625, 0.0, 0.1875, 0.25]);
    let color = paint.sample(0.75, [0.5, 0.5]);
    assert_eq!(color[0], 0.03125);
    assert_eq!(color[2], 0.09375);
    assert_eq!(color[1], 64.0 / 255.0);
    assert_eq!(color[3], 0.125 + 64.0 / 255.0);
    assert_eq!(paint.sample(-1.0, [1.0, 0.0]), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(paint.sample(2.0, [1.0, 0.0]), [0.0; 4]);
}

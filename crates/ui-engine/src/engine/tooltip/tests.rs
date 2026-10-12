use super::*;

#[test]
fn native_font_height_uses_minimum_only_for_first_line_and_pitch_for_newlines() {
    let default = WIDTH_EXTRA;
    assert_eq!(
        text_height(default, TEXT_PITCH as f32, 1),
        TEXT_PITCH as f32
    );
    assert_eq!(
        text_height(default, TEXT_PITCH as f32, 3),
        (TEXT_PITCH * 3) as f32
    );
    let smaller = 0.5;
    let wrap = TEXT_PITCH as f32 * smaller;
    let minimum = ((smaller - default) * 0.5 + default) * BOX_EXTRA;
    assert!(minimum > wrap * smaller);
    assert_eq!(text_height(smaller, wrap, 1), minimum);
    assert_eq!(text_height(smaller, wrap, 2), minimum + wrap * smaller);
    let larger = 2.0;
    let wrap = TEXT_PITCH as f32 * larger;
    assert_eq!(text_height(larger, wrap, 2), wrap * larger * 2.0);
}

#[test]
fn native_mouse_offset_and_integer_box_extents() {
    let anchor = [30.0, 50.0];
    let text = [24.75, TEXT_PITCH as f32];
    let rect = box_rect(anchor, text, [300.0, 200.0]);
    assert_eq!(rect.x, f64::from(anchor[0] + MOUSE_OFFSET[0]));
    assert_eq!(rect.y, f64::from(anchor[1] + MOUSE_OFFSET[1]));
    assert_eq!(rect.w, f64::from(text[0].ceil() + WIDTH_EXTRA + BOX_EXTRA));
    assert_eq!(rect.h, f64::from(text[1] + BOX_EXTRA));
}

#[test]
fn right_overflow_flips_left_of_pointer_not_to_screen_edge() {
    let anchor = [195.0, 50.0];
    let rect = box_rect(anchor, [24.0, TEXT_PITCH as f32], [200.0, 200.0]);
    assert_eq!(rect.x + rect.w, f64::from(anchor[0] - MOUSE_OFFSET[0]));
    assert_eq!(rect.y, f64::from(anchor[1] + MOUSE_OFFSET[1]));
}

#[test]
fn bottom_overflow_moves_box_up_without_changing_horizontal_offset() {
    let viewport = [300.0, 200.0];
    let anchor = [30.0, viewport[1]];
    let rect = box_rect(anchor, [24.0, TEXT_PITCH as f32], viewport);
    assert_eq!(rect.x, f64::from(anchor[0] + MOUSE_OFFSET[0]));
    assert_eq!(rect.y + rect.h, f64::from(viewport[1]));
}

#[test]
fn oversized_box_centers_above_pointer_and_native_does_not_top_clamp() {
    let anchor = [100.0, 4.0];
    let rect = box_rect(anchor, [220.0, TEXT_PITCH as f32], [200.0, 200.0]);
    assert_eq!(rect.x + rect.w * 0.5, f64::from(anchor[0]));
    assert_eq!(rect.y + rect.h, f64::from(anchor[1]));
    assert!(rect.y < 0.0);
    let short = box_rect(anchor, [24.0, TEXT_PITCH as f32], [300.0, 200.0]);
    assert_eq!(short.y, f64::from(anchor[1] + MOUSE_OFFSET[1]));
    assert!(short.y < 0.0);
}

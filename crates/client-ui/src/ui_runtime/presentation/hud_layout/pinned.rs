//! Pinned presentation tables and timing helpers: sprite selection, blink
//! phases, and recorded color approximations.

pub use ui::gui_scale;

/// Vanilla survival hotbar width in GUI px (start cap + nine slots + end cap).
pub(super) const HOTBAR_WIDTH: f32 = 182.0;
/// Fixed height of the bottom-anchored HUD stack in GUI px, measured from the
/// selected-item label zone top down to the hotbar's bottom edge.
pub(super) const BOTTOM_STACK_HEIGHT: f32 = 59.0;
/// Boss bar tint per authoritative color. The carried track sprites are the
/// official Bedrock progress textures; these multipliers are a recorded
/// approximation of the reference bar hues pending the native gallery
/// (RebeccaPurple is exact by definition).
pub const BOSS_TINTS: [(ui::BossColor, [u8; 4]); 8] = [
    (ui::BossColor::Pink, [255, 105, 180, 255]),
    (ui::BossColor::Blue, [85, 85, 255, 255]),
    (ui::BossColor::Red, [255, 85, 85, 255]),
    (ui::BossColor::Green, [85, 255, 85, 255]),
    (ui::BossColor::Yellow, [255, 255, 85, 255]),
    (ui::BossColor::Purple, [170, 0, 170, 255]),
    (ui::BossColor::RebeccaPurple, [102, 51, 153, 255]),
    (ui::BossColor::White, [255, 255, 255, 255]),
];

/// Durability hue: green at full durability sweeping to red, matching the
/// reference's HSV ramp (hue = fraction / 3, full saturation and value).
pub(super) fn hsv_to_rgb(hue: f32) -> [u8; 4] {
    let hue = hue.clamp(0.0, 1.0) * 6.0;
    let sector = hue.floor() as u32 % 6;
    let fraction = hue - hue.floor();
    let ascending = (fraction * 255.0) as u8;
    let descending = 255 - ascending;
    match sector {
        0 => [255, ascending, 0, 255],
        1 => [descending, 255, 0, 255],
        2 => [0, 255, ascending, 255],
        3 => [0, descending, 255, 255],
        4 => [ascending, 0, 255, 255],
        _ => [255, 0, descending, 255],
    }
}

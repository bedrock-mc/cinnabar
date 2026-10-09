//! Item-state projection shared by gameplay and UI.

/// Maximum held-use duration for native bows and tridents.
pub const LONG_WEAPON_USE_TICKS: u32 = 72_000;

/// The ranged-weapon icon follows the quadratic draw-power curve,
/// independently of the attachable's charge pose.
pub fn ranged_animation_frame(elapsed: Option<u32>) -> u32 {
    let Some(elapsed) = elapsed else {
        return 0;
    };
    let seconds = elapsed as f32 / 20.0;
    let power = ((seconds * seconds + 2.0 * seconds) / 3.0).min(1.0);
    (3.0 * power * 0.99) as u32 + 1
}

/// Vanilla crossbow animation frame, including loaded projectile art.
pub fn crossbow_animation_frame(
    elapsed: Option<u32>,
    duration: u32,
    projectile: Option<&str>,
    offhand_firework: bool,
) -> u32 {
    if let Some(elapsed) = elapsed.filter(|_| duration > 0) {
        let fraction = elapsed as f32 / duration as f32;
        let power = ((fraction * fraction + 2.0 * fraction) / 3.0).min(1.0);
        let frame = (power * 0.99 * 5.0) as u32;
        if frame >= 4 && power < 1.0 && offhand_firework {
            5
        } else {
            frame
        }
    } else {
        projectile.map_or(0, |projectile| {
            if projectile == "minecraft:arrow" {
                4
            } else {
                5
            }
        })
    }
}

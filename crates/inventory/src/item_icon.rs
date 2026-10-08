//! Item-state projection shared by gameplay and UI.

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

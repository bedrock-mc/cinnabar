//! Native HUD presentation shared by authored client and browser screens.

mod model;
mod motion;
mod paint;
mod pinned;
mod status;

pub use model::{
    HeartVariant, HudEffect, heart_variant, hunger_effect_active, regeneration_active,
};
pub use paint::{
    CROSSHAIR_SIDE, CROSSHAIR_TEXTURE, Cell, HudPaint, HudPaintTarget, SheetSprite, paint,
};
pub use pinned::effect_icon_role;
pub use status::{HeartPaint, StatusPaintInput, capture_status_hud};

/// The built-in Java-styled HUD pack: `(pack path, namespace, bytes)`, layered
/// under every server pack.
pub const JAVA_HUD_PACK: [(&str, &str, &[u8]); 4] = [
    (
        "ui/_global_variables.json",
        "",
        include_bytes!("../../../assets/java-hud/ui/_global_variables.json"),
    ),
    (
        "ui/chat_screen.json",
        "chat",
        include_bytes!("../../../assets/java-hud/ui/chat_screen.json"),
    ),
    (
        "ui/hud_screen.json",
        "hud",
        include_bytes!("../../../assets/java-hud/ui/hud_screen.json"),
    ),
    (
        "ui/scoreboards.json",
        "scoreboard",
        include_bytes!("../../../assets/java-hud/ui/scoreboards.json"),
    ),
];

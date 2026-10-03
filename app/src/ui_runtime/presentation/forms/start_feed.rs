//! The start screen's service-fed bindings: messaging art on the Play and
//! Store buttons, the inbox badge, the Realms invite count and the live-event
//! button with its countdown caption.

use std::time::{SystemTime, UNIX_EPOCH};

use json_ui::{DataSource, Scalar};

use crate::menu::{ButtonArt, MenuView};

fn text(value: impl Into<String>) -> Scalar {
    Scalar::Text(value.into())
}

pub(super) fn bind(view: &MenuView, data: &mut DataSource) {
    let home = &view.feeds.home;
    button_art(data, "play", home.play_art.as_ref());
    button_art(data, "store", home.store_art.as_ref());
    // The inbox and friends drawer need an Xbox Live session.
    let signed_in = view.auth_state == crate::menu::auth::AuthState::Authenticated;
    data.set_global("#inbox_enabled", Scalar::Bool(signed_in));
    data.set_global("#friends_drawer_button_enabled", Scalar::Bool(signed_in));
    friends_button(view, data);
    data.set_global(
        "#unread_notification_icon_visibility",
        Scalar::Bool(home.inbox_unread > 0),
    );
    let invites = if home.realm_invites > 0 {
        home.realm_invites.to_string()
    } else {
        String::new()
    };
    data.set_global("#realms_notification_count", text(invites));
    let Some(event) = &home.live_event else {
        return;
    };
    for name in ["#gathering_enabled", "#gathering_button_enabled"] {
        data.set_global(name, Scalar::Bool(true));
    }
    data.set_global("#gathering_button_text", text(event.button_text.clone()));
    data.set_global("#gathering_badge", text(event.badge_path.clone()));
    data.set_global("#gathering_badge_file_system", text("RawPath"));
    data.set_global(
        "#gathering_badge_visible",
        Scalar::Bool(!event.badge_path.is_empty()),
    );
    data.set_global("#gathering_gif_badge", Scalar::Bool(false));
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    let caption = if event.countdown {
        countdown_caption(&event.caption, event.start_unix, now)
    } else {
        event.caption.clone()
    };
    data.set_global("#gathering_countdown_text", text(caption));
}

/// The friends drawer button: the social glyph and the count of friends in a
/// joinable world (the label's exact wording needs native measurement).
fn friends_button(view: &MenuView, data: &mut DataSource) {
    data.set_factory(
        "social_icons_factory",
        vec![json_ui::FactoryItem::new("social_button_control", 0.0)],
    );
    data.set_global("#social_icon_content", Scalar::Num(1.0));
    data.set_global(
        "#social_icon",
        text("textures/ui/socialbuttonicon/social-default-icon"),
    );
    data.set_global(
        "#social_icon_hovered",
        text("textures/ui/socialbuttonicon/social-hover-icon"),
    );
    data.set_global("#social_button_text", text(view.friends.len().to_string()));
}

/// A main button's art layers; the hover layers stay hidden until hovered.
fn button_art(data: &mut DataSource, button: &str, art: Option<&ButtonArt>) {
    let visible = art.is_some_and(|art| !art.default_background.is_empty());
    data.set_global(
        format!("#{button}_button_art_visible"),
        Scalar::Bool(visible),
    );
    let Some(art) = art else {
        return;
    };
    for (field, color) in [
        ("art_label_color", "DefaultTextColor"),
        ("banner_text_color", "BannerTextColor"),
    ] {
        if let Some(rgb) = art.colors.get(color) {
            data.set_global(
                format!("#{button}_button_{field}"),
                Scalar::Json(serde_json::json!(
                    rgb.map(|channel| f64::from(channel) / 255.0)
                )),
            );
        }
    }
    data.set_global(
        format!("#{button}_button_banner_texture"),
        text(art.banner_texture.clone()),
    );
    data.set_global(
        format!("#{button}_button_banner_texture_source"),
        text("RawPath"),
    );
    for (layer, path) in [
        ("default_bg", &art.default_background),
        ("hover_bg", &art.hover_background),
        ("default_fg", &art.default_foreground),
        ("hover_fg", &art.hover_foreground),
    ] {
        data.set_global(
            format!("#{button}_button_art_{layer}_path"),
            text(path.clone()),
        );
    }
    data.set_global(
        format!("#{button}_button_art_default_alpha"),
        Scalar::Num(1.0),
    );
    data.set_global(
        format!("#{button}_button_art_hover_alpha"),
        Scalar::Num(0.0),
    );
    data.set_global(
        format!("#{button}_button_banner_visible"),
        Scalar::Bool(!art.banner.is_empty()),
    );
    data.set_global(
        format!("#{button}_button_banner_text"),
        text(art.banner.clone()),
    );
}

/// The caption with the time left until the event starts; the format needs native measurement.
fn countdown_caption(caption: &str, start_unix: i64, now_unix: i64) -> String {
    let left = start_unix - now_unix;
    if left <= 0 {
        return caption.to_owned();
    }
    let (days, hours, minutes) = (left / 86_400, left % 86_400 / 3_600, left % 3_600 / 60);
    let remaining = if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{}m", minutes.max(1))
    };
    if caption.is_empty() {
        remaining
    } else {
        format!("{caption} {remaining}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdowns_shorten_as_the_start_nears() {
        assert_eq!(
            countdown_caption("Live in", 86_400 + 7_200, 0),
            "Live in 1d 2h"
        );
        assert_eq!(countdown_caption("", 3_700, 0), "1h 1m");
        assert_eq!(countdown_caption("Live", 30, 0), "Live 1m");
        assert_eq!(countdown_caption("Live now", 0, 10), "Live now");
    }
}

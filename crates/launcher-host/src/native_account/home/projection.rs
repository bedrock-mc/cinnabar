//! The existing Home marketing projection, independent from authentication or I/O.

use bedrock_services::minecraft::player_messaging::{Message, MessageItem, Session};
use std::{collections::HashSet, io};

/// Flattens top-level then category messages with the original first-valid-instance policy.
pub(super) fn flatten(session: Session) -> io::Result<(Vec<bridge::Message>, bridge::Inbox)> {
    let mut inbox = bridge::Inbox {
        total: count(session.inbox_summary.total)?,
        ..Default::default()
    };
    let mut all = session.messages;
    for category in session.inbox_summary.categories {
        let unread = count(category.unread)?;
        inbox.unread = inbox.unread.checked_add(unread).ok_or_else(invalid_count)?;
        inbox.categories.push(bridge::InboxCategory {
            kind: category.info.kind,
            unread,
        });
        all.extend(category.messages);
    }
    let mut seen = HashSet::new();
    let messages = all
        .into_iter()
        .filter(|message| {
            !message.id.is_empty()
                && !message.surface.is_empty()
                && !message.template.is_empty()
                && seen.insert((message.id.clone(), message.instance_id.clone()))
        })
        .map(message)
        .collect();
    Ok((messages, inbox))
}

/// Projects known display fields without following button links or fetching artwork.
fn message(wire: Message) -> bridge::Message {
    let mut message = bridge::Message {
        id: wire.id,
        instance_id: wire.instance_id,
        report_id: wire.report_id,
        surface: wire.surface,
        template: wire.template,
        category: wire.inbox_category,
        status: wire.status,
        received: wire.date_received,
        sender: wire.sender,
        header: wire.text.header,
        body: wire.text.body,
        banner: wire.text.banner,
        colors: wire
            .colors
            .into_iter()
            .map(|(name, color)| (name, [color.rgb.r, color.rgb.g, color.rgb.b]))
            .collect(),
        images: wire
            .images
            .into_iter()
            .filter(|(_, image)| artwork_url(&image.url))
            .map(|(id, image)| bridge::MessageImage {
                id,
                url: image.url,
                path: String::new(),
            })
            .collect(),
        buttons: wire
            .buttons
            .into_iter()
            .map(|(id, button)| bridge::MessageButton {
                id,
                text: button.text,
                link: button.link,
                action: button.action.to_lowercase(),
            })
            .collect(),
        ..Default::default()
    };
    for item in wire.items {
        item_fields(&mut message, item);
    }
    message.images.sort_by(|a, b| a.id.cmp(&b.id));
    message.buttons.sort_by(|a, b| a.id.cmp(&b.id));
    message
}

/// Keeps the original first nonempty text and every usable item image/button.
fn item_fields(message: &mut bridge::Message, item: MessageItem) {
    if message.banner.is_empty() {
        message.banner = item.sale_banner_text();
    }
    if message.sub_title.is_empty() {
        message.sub_title = item.sub_title;
    }
    if artwork_url(&item.image.url) {
        message.images.push(bridge::MessageImage {
            id: item.image.id,
            url: item.image.url,
            path: String::new(),
        });
    }
    if !item.button.text.is_empty() || !item.button.link.is_empty() {
        message.buttons.push(bridge::MessageButton {
            id: item.button.id,
            text: item.button.text,
            link: item.button.link,
            action: item.button.action.to_lowercase(),
        });
    }
}

/// Admits display metadata as Go does; the existing image downloader still enforces public egress.
fn artwork_url(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some_and(|host| !host.is_empty())
            && url.username().is_empty()
            && url.password().is_none()
    })
}

/// Applies the existing Rust bridge count representation after the Go-shaped service model.
fn count(value: i64) -> io::Result<u32> {
    u32::try_from(value).map_err(|_| invalid_count())
}

/// Keeps invalid service counts out of the user-visible recovery copy.
fn invalid_count() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid native Home counts")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    /// Both actual Go and native projection consume the same original synthetic service response.
    #[test]
    fn native_home_projection_preserves_the_original_go_fixture() {
        #[derive(Deserialize)]
        struct Envelope {
            result: Session,
        }
        let fixture = include_str!("../../../../../core/catalog/native_home_session.json");
        let envelope: Envelope = serde_json::from_str(fixture).unwrap();
        let (messages, inbox) = flatten(envelope.result).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].id, "m1");
        assert_eq!(messages[1].surface, "InboxMessage");
        assert_eq!(messages[0].buttons[0].action, "internal");
        assert_eq!(messages[0].images[0].url, "https://cdn.test/t.png");
        assert_eq!(inbox.total, 3);
        assert_eq!(inbox.unread, 2);
        assert_eq!(inbox.categories[0].kind, "News");
    }
}

//! Presented text shared by the screens and the retained HUD: chat rows resolved against the
//! language tables, and every server string bounded before layout.

use std::{borrow::Cow, sync::Arc};

use ui::{ChatMessage, ChatMessageKind};

/// The most bytes of one server string the client presents.
pub const MAX_PRESENTED_TEXT_BYTES: usize = 512;

/// The presented text for one chat row. `Translation`/command-output rows
/// resolve their key against `translate` and splice their ordered parameters
/// (an unknown key presents verbatim, as the vanilla client does); a `Chat`
/// row with a source gains the `<name>` prefix via `chat.type.text`. Other
/// kinds present their message unchanged. Bedrock `§` codes in the result are
/// parsed later at layout time.
pub fn resolve_chat_line<'a>(
    node: &'a ChatMessage,
    translate: impl Fn(&str) -> Option<Arc<str>>,
) -> Cow<'a, str> {
    match node.kind {
        ChatMessageKind::Translation => {
            let template = json_ui::localize_text_chars(&node.message, &translate);
            let arguments = node
                .parameters
                .iter()
                .map(|parameter| {
                    protocol::localize_parameter_prefix(parameter, &translate, usize::MAX)
                        .into_owned()
                })
                .collect::<Vec<_>>();
            Cow::Owned(protocol::format_translation_chars(template, &arguments))
        }
        ChatMessageKind::Chat => match node.source.as_deref() {
            Some(source) if !source.is_empty() => {
                let template = translate("chat.type.text").unwrap_or_else(|| Arc::from("<%s> %s"));
                Cow::Owned(protocol::format_translation(
                    &template,
                    &[source.to_owned(), node.message.as_ref().to_owned()],
                ))
            }
            _ => Cow::Borrowed(node.message.as_ref()),
        },
        _ => Cow::Borrowed(node.message.as_ref()),
    }
}

/// Highlights mention tokens after label binding and preserves their surrounding formatting.
pub fn highlight_mentions(text: &str, mention_code: char) -> Cow<'_, str> {
    if !text.contains('@') {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 16);
    let mut chars = text.char_indices().peekable();
    let mut style = ui::TextStyle::default();
    let mut color = None;
    let mut last_end = 0;
    let mut boundary = true;
    while let Some((idx, ch)) = chars.next() {
        if ch == '§' {
            if let Some((_, code)) = chars.next() {
                if ui::BedrockColor::from_code(code).is_some() {
                    color = Some(code);
                } else {
                    match code {
                        'k' => style.obfuscated = true,
                        'l' => style.bold = true,
                        'o' => style.italic = true,
                        'r' => {
                            style = ui::TextStyle::default();
                            color = None;
                        }
                        _ => {}
                    }
                }
            }
            continue;
        }
        if ch == '@' && boundary {
            let mut end = idx + 1;
            while let Some(&(next_idx, next_ch)) = chars.peek() {
                if !next_ch.is_alphanumeric() && next_ch != '_' {
                    break;
                }
                chars.next();
                end = next_idx + next_ch.len_utf8();
            }
            if end > idx + 1 {
                out.push_str(&text[last_end..idx]);
                out.push('§');
                out.push(mention_code);
                out.push_str(&text[idx..end]);
                if let Some(code) = color {
                    // Color codes preserve the other active style flags.
                    out.push('§');
                    out.push(code);
                } else {
                    out.push_str("§r");
                    if style.obfuscated {
                        out.push_str("§k");
                    }
                    if style.bold {
                        out.push_str("§l");
                    }
                    if style.italic {
                        out.push_str("§o");
                    }
                }
                last_end = end;
            }
        }
        boundary = ch.is_whitespace() || ch == ':' || ch == '<';
    }
    if last_end == 0 {
        Cow::Borrowed(text)
    } else {
        out.push_str(&text[last_end..]);
        Cow::Owned(out)
    }
}

/// Applies the saved default color and then mention coloring to a decoded label.
pub(crate) fn format_chat_label(text: &str, color: Option<char>, mention: Option<char>) -> String {
    let colored = color.map(|code| {
        let mut result = format!("§{code}");
        let mut chars = text.chars();
        while let Some(ch) = chars.next() {
            result.push(ch);
            if ch == '§'
                && let Some(next) = chars.next()
            {
                result.push(next);
                if next == 'r' {
                    result.push('§');
                    result.push(code);
                }
            }
        }
        result
    });
    match mention {
        Some(mention) => {
            highlight_mentions(colored.as_deref().unwrap_or(text), mention).into_owned()
        }
        None => colored.unwrap_or_else(|| text.to_owned()),
    }
}

/// `value` cut at a character boundary to [`MAX_PRESENTED_TEXT_BYTES`].
pub fn bounded_visible_text(value: &str) -> &str {
    bounded_text(value, MAX_PRESENTED_TEXT_BYTES)
}

/// `value` cut at a character boundary to at most `max_bytes`.
pub fn bounded_text(value: &str, max_bytes: usize) -> &str {
    &value[..value.floor_char_boundary(max_bytes)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(
        kind: ChatMessageKind,
        source: Option<&str>,
        text: &str,
        parameters: &[&str],
    ) -> ChatMessage {
        ChatMessage {
            fifo_sequence: 0,
            received_millis: 0,
            kind,
            source: source.map(Arc::from),
            message: Arc::from(text),
            parameters: parameters.iter().map(|value| Arc::from(*value)).collect(),
        }
    }

    #[test]
    fn mentions_are_highlighted_with_selected_color() {
        let text = "Hello @PlayerName how are you?";
        let formatted = highlight_mentions(text, 'e');
        assert_eq!(formatted, "Hello §e@PlayerName§r how are you?");
    }

    #[test]
    fn emails_or_inner_ats_are_not_mentions() {
        let text = "Contact test@example.com for info";
        let formatted = highlight_mentions(text, 'e');
        assert_eq!(formatted, "Contact test@example.com for info");
    }

    #[test]
    fn mention_highlighting_preserves_surrounding_styles() {
        let text = "§c§l§oAlert @Alex still red";
        let formatted = highlight_mentions(text, 'e');
        let spans = ui::parse_bedrock_text(&formatted, usize::MAX).unwrap();
        assert_eq!(spans.plain_text(), "Alert @Alex still red");
        let after = spans.last().unwrap();
        assert_eq!(after.style.color, ui::BedrockColor::Red);
        assert!(after.style.bold && after.style.italic);
    }

    #[test]
    fn white_mentions_and_explicit_white_survive_a_selected_chat_color() {
        let text = format_chat_label("Hello @Alex§r tail §fwhite", Some('a'), Some('f'));
        let spans = ui::parse_bedrock_text(&text, usize::MAX).unwrap();
        assert_eq!(spans.plain_text(), "Hello @Alex tail white");
        assert_eq!(spans[0].style.color, ui::BedrockColor::Green);
        assert_eq!(spans[1].style.color, ui::BedrockColor::White);
        assert_eq!(spans[2].style.color, ui::BedrockColor::Green);
        assert_eq!(spans[3].style.color, ui::BedrockColor::White);
    }

    #[test]
    fn mentions_restore_default_styles_and_handle_formatting_at_boundaries() {
        for input in ["§l§o@Alex tail", "§l§oHello @Alex tail"] {
            let text = highlight_mentions(input, 'e');
            let spans = ui::parse_bedrock_text(&text, usize::MAX).unwrap();
            let after = spans.last().unwrap();
            assert_eq!(after.style.color, ui::BedrockColor::Base);
            assert!(after.style.bold && after.style.italic);
            assert!(
                spans.iter().any(|span| span.text.contains("@Alex")
                    && span.style.color == ui::BedrockColor::Yellow)
            );
        }
    }

    #[test]
    fn translation_row_resolves_key_and_splices_ordered_parameters() {
        let node = message(
            ChatMessageKind::Translation,
            None,
            "death.attack.player",
            &["Legolas", "Gimli"],
        );
        let resolved = resolve_chat_line(&node, |key| {
            (key == "death.attack.player").then(|| Arc::from("%1$s was slain by %2$s"))
        });
        assert_eq!(resolved, "Legolas was slain by Gimli");
    }

    #[test]
    fn death_message_uses_the_active_entity_name_and_preserves_player_names() {
        let node = message(
            ChatMessageKind::Translation,
            None,
            "death.attack.mob",
            &["notchyves", "%entity.zombie.name"],
        );
        assert_eq!(
            resolve_chat_line(&node, |key| match key {
                "death.attack.mob" => Some(Arc::from("%1$s was slain by %2$s")),
                "entity.zombie.name" => Some(Arc::from("§aZombie§r")),
                "notchyves" => Some(Arc::from("must stay literal")),
                _ => None,
            }),
            "notchyves was slain by §aZombie§r"
        );
    }

    #[test]
    fn marked_translation_keys_and_arguments_resolve() {
        let node = message(
            ChatMessageKind::Translation,
            None,
            "§e%multiplayer.player.joined",
            &["Alex"],
        );
        let resolved = resolve_chat_line(&node, |key| {
            (key == "multiplayer.player.joined").then(|| Arc::from("%s joined the game"))
        });
        assert_eq!(resolved, "§eAlex joined the game");
        let node = message(
            ChatMessageKind::Translation,
            None,
            "test.key",
            &["%menu.play"],
        );
        let resolved = resolve_chat_line(&node, |key| match key {
            "test.key" => Some(Arc::from("Selected %s")),
            "menu.play" => Some(Arc::from("Play")),
            _ => None,
        });
        assert_eq!(resolved, "Selected Play");
    }

    #[test]
    fn translation_arguments_keep_unmarked_names_and_literal_percent_text() {
        let node = message(
            ChatMessageKind::Translation,
            None,
            "wrap",
            &[
                "menu.play",
                "100% literal %menu.play",
                "%missing",
                "%menu.play",
            ],
        );
        assert_eq!(
            resolve_chat_line(&node, |key| match key {
                "wrap" => Some(Arc::from("%s | %s | %s | %s")),
                "menu.play" => Some(Arc::from("Play")),
                _ => None,
            }),
            "menu.play | 100% literal %menu.play | %missing | Play"
        );
    }

    #[test]
    fn zero_argument_translations_format_percent_escapes_while_literal_rows_stay_literal() {
        let translated = message(ChatMessageKind::Translation, None, "progress", &[]);
        assert_eq!(
            resolve_chat_line(&translated, |key| {
                (key == "progress").then(|| Arc::from("100%% complete"))
            }),
            "100% complete"
        );
        for kind in [ChatMessageKind::Chat, ChatMessageKind::System] {
            let literal = message(kind, None, "100%% complete", &[]);
            assert_eq!(resolve_chat_line(&literal, |_| None), "100%% complete");
        }
    }

    #[test]
    fn translation_expansion_stops_lookup_after_output_is_full() {
        let expanded: Arc<str> = "a".repeat(protocol::MAX_RAW_TEXT_OUTPUT_BYTES * 2).into();
        let calls = std::cell::Cell::new(0);
        let node = message(ChatMessageKind::Translation, None, "%x %x %x", &[]);
        let resolved = resolve_chat_line(&node, |key| {
            calls.set(calls.get() + 1);
            assert_eq!(calls.get(), 1, "the first value fills the formatted output");
            (key == "x").then(|| Arc::clone(&expanded))
        });
        assert_eq!(resolved, protocol::format_translation(&expanded, &[]));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn translation_streams_formatting_across_localization_chunks() {
        for (text, value, arguments, expected) in [
            ("%x$s", "%1", vec!["🌍"], "🌍".to_owned()),
            ("%x%s", "%", vec!["unused"], "%s".to_owned()),
            ("%x!", "%.2f", vec!["1.25"], "1.25!".to_owned()),
            ("%x!", "%.2", vec![], "%.2!".to_owned()),
            ("%x!", "§a🌍%%", vec![], "§a🌍%!".to_owned()),
            ("%x!", "", vec![], "!".to_owned()),
            ("%x%1$s", "", vec!["🌍"], "🌍".to_owned()),
        ] {
            let node = message(ChatMessageKind::Translation, None, text, &arguments);
            assert_eq!(
                resolve_chat_line(&node, |key| (key == "x").then(|| Arc::from(value))),
                expected
            );
        }
    }

    #[test]
    fn translation_consumes_template_past_the_output_budget_when_formatting_shrinks_it() {
        for value in [
            format!("{}🌍", "%%".repeat(protocol::MAX_RAW_TEXT_OUTPUT_BYTES / 2)),
            format!("{}🌍", "%s".repeat(protocol::MAX_RAW_TEXT_OUTPUT_BYTES)),
        ] {
            let node = message(ChatMessageKind::Translation, None, "%x!", &[]);
            let expected = protocol::format_translation(&format!("{value}!"), &[]);
            assert!(expected.ends_with("🌍!"));
            assert_eq!(
                resolve_chat_line(&node, |key| (key == "x").then(|| Arc::from(value.as_str()))),
                expected
            );
        }
    }

    #[test]
    fn unknown_translation_key_presents_verbatim() {
        let node = message(ChatMessageKind::Translation, None, "death.attack.void", &[]);
        let resolved = resolve_chat_line(&node, |_| None);
        assert_eq!(resolved, "death.attack.void");
    }

    #[test]
    fn chat_row_prepends_the_source_name() {
        let node = message(ChatMessageKind::Chat, Some("Steve"), "hello", &[]);
        let resolved = resolve_chat_line(&node, |_| None);
        assert_eq!(resolved, "<Steve> hello");
    }

    #[test]
    fn chat_row_without_a_source_is_unchanged() {
        let node = message(ChatMessageKind::Chat, None, "server broadcast", &[]);
        let resolved = resolve_chat_line(&node, |_| None);
        assert_eq!(resolved, "server broadcast");
    }

    #[test]
    fn system_row_is_never_translated() {
        let node = message(ChatMessageKind::System, None, "chat.type.text", &["x"]);
        let resolved = resolve_chat_line(&node, |_| Some(Arc::from("<%s> %s")));
        assert_eq!(resolved, "chat.type.text");
    }
}

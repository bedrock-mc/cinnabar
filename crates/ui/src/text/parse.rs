//! Bedrock `§` formatting-code parser.

use super::{BedrockColor, MAX_TEXT_SPANS, TextError, TextSpan, TextSpans, TextStyle};

pub fn parse_bedrock_text(text: &str, max_bytes: usize) -> Result<TextSpans, TextError> {
    parse_bedrock_text_with_style(text, max_bytes, TextStyle::default())
}

pub(super) fn parse_bedrock_text_with_style(
    text: &str,
    max_bytes: usize,
    base_style: TextStyle,
) -> Result<TextSpans, TextError> {
    if text.len() > max_bytes {
        return Err(TextError::TextBytesExceeded {
            actual: text.len(),
            limit: max_bytes,
        });
    }

    let mut spans = Vec::new();
    let mut buffer = String::new();
    let mut style = base_style;
    let mut buffer_style = base_style;
    let mut characters = NormalizedChars::new(text).peekable();
    while let Some(character) = characters.next() {
        if character != '§' {
            if style != buffer_style {
                push_span(&mut spans, &mut buffer, buffer_style)?;
                buffer_style = style;
            }
            buffer.push(character);
            continue;
        }

        let Some(code) = characters.peek().copied() else {
            if style != buffer_style {
                push_span(&mut spans, &mut buffer, buffer_style)?;
                buffer_style = style;
            }
            buffer.push(character);
            continue;
        };
        // Vanilla consumes `§` plus any following character (a newline too); an
        // unknown code draws nothing, which servers use for hidden markers.
        let Some(change) = formatting_change(code) else {
            characters.next();
            continue;
        };

        characters.next();
        match change {
            FormattingChange::Color(color) => {
                style.color = color;
            }
            FormattingChange::Obfuscated => style.obfuscated = true,
            FormattingChange::Bold => style.bold = true,
            FormattingChange::Italic => style.italic = true,
            FormattingChange::Reset => style = base_style,
        }
    }
    push_span(&mut spans, &mut buffer, buffer_style)?;
    Ok(TextSpans(spans))
}

struct NormalizedChars<'a> {
    characters: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> NormalizedChars<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            characters: text.chars().peekable(),
        }
    }
}

impl Iterator for NormalizedChars<'_> {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        let character = self.characters.next()?;
        if character == '\r' && self.characters.peek() == Some(&'\n') {
            self.characters.next();
            return Some('\n');
        }
        Some(character)
    }
}

/// Commits one contiguous run once its visible style changes.
fn push_span(
    spans: &mut Vec<TextSpan>,
    buffer: &mut String,
    style: TextStyle,
) -> Result<(), TextError> {
    if buffer.is_empty() {
        return Ok(());
    }
    let actual = spans
        .len()
        .checked_add(1)
        .ok_or(TextError::FixedPointOverflow)?;
    if actual > MAX_TEXT_SPANS {
        return Err(TextError::SpanLimitExceeded {
            actual,
            limit: MAX_TEXT_SPANS,
        });
    }
    spans.push(TextSpan {
        text: std::mem::take(buffer).into_boxed_str(),
        style,
    });
    Ok(())
}

#[derive(Clone, Copy)]
enum FormattingChange {
    Color(BedrockColor),
    Obfuscated,
    Bold,
    Italic,
    Reset,
}

/// Codes are case-sensitive; an unlisted digit or `a`-`w` falls back to white.
fn formatting_change(code: char) -> Option<FormattingChange> {
    use BedrockColor as Color;
    use FormattingChange as Change;
    Some(match code {
        '0' => Change::Color(Color::Black),
        '1' => Change::Color(Color::DarkBlue),
        '2' => Change::Color(Color::DarkGreen),
        '3' => Change::Color(Color::DarkAqua),
        '4' => Change::Color(Color::DarkRed),
        '5' => Change::Color(Color::DarkPurple),
        '6' => Change::Color(Color::Gold),
        '7' => Change::Color(Color::Gray),
        '8' => Change::Color(Color::DarkGray),
        '9' => Change::Color(Color::Blue),
        'a' => Change::Color(Color::Green),
        'b' => Change::Color(Color::Aqua),
        'c' => Change::Color(Color::Red),
        'd' => Change::Color(Color::LightPurple),
        'e' => Change::Color(Color::Yellow),
        'f' => Change::Color(Color::White),
        'g' => Change::Color(Color::MinecoinGold),
        'h' => Change::Color(Color::MaterialQuartz),
        'i' => Change::Color(Color::MaterialIron),
        'j' => Change::Color(Color::MaterialNetherite),
        'm' => Change::Color(Color::MaterialRedstone),
        'n' => Change::Color(Color::MaterialCopper),
        'p' => Change::Color(Color::MaterialGold),
        'q' => Change::Color(Color::MaterialEmerald),
        's' => Change::Color(Color::MaterialDiamond),
        't' => Change::Color(Color::MaterialLapis),
        'u' => Change::Color(Color::MaterialAmethyst),
        'v' => Change::Color(Color::MaterialResin),
        'k' => Change::Obfuscated,
        'l' => Change::Bold,
        'o' => Change::Italic,
        'r' => Change::Reset,
        'w' => Change::Color(Color::PartyBlue),
        _ => return None,
    })
}

#[cfg(test)]
mod review_tests {
    use super::*;

    #[test]
    fn review_redundant_style_changes_have_bounded_parsing_work() {
        let text = "a§l§r".repeat(1_000_000);
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = parse_bedrock_text(&text, text.len()).unwrap();
            send.send((result.0.len(), result.0[0].text.len())).unwrap();
        });
        assert_eq!(
            receive
                .recv_timeout(std::time::Duration::from_secs(3))
                .unwrap(),
            (1, 1_000_000)
        );
    }
}

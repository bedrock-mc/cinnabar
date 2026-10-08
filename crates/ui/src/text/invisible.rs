//! Code points that lay out as nothing instead of a replacement box.

/// Controls (other than newline) and default-ignorable format characters such as
/// zero-width joiners and variation selectors, which servers append to symbols.
pub(super) fn is_invisible(character: char) -> bool {
    if character == '\n' {
        return false;
    }
    character.is_control()
        || matches!(
            u32::from(character),
            0x00ad
                | 0x034f
                | 0x061c
                | 0x115f..=0x1160
                | 0x17b4..=0x17b5
                | 0x180b..=0x180f
                | 0x200b..=0x200f
                | 0x2028..=0x202e
                | 0x2060..=0x206f
                | 0x3164
                | 0xfe00..=0xfe0f
                | 0xfeff
                | 0xffa0
                | 0xfff9..=0xfffb
                | 0xe0000..=0xe007f
                | 0xe0100..=0xe01ef
        )
}

#[cfg(test)]
mod tests {
    use super::is_invisible;

    #[test]
    fn format_and_control_characters_are_invisible() {
        for character in [
            '\u{e0100}',
            '\u{e01ef}',
            '\u{200b}',
            '\u{200d}',
            '\u{fe0f}',
            '\u{feff}',
            '\t',
            '\u{7f}',
            '\u{85}',
        ] {
            assert!(is_invisible(character), "{character:?}");
        }
    }

    #[test]
    fn visible_text_and_newline_are_kept() {
        for character in ['a', ' ', '\n', '§', '❤', '\u{2003}', '中', '\u{fffd}'] {
            assert!(!is_invisible(character), "{character:?}");
        }
    }
}

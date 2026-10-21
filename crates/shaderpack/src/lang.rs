//! A pack's option-menu text from `lang/<code>.lang`, resolved with Iris's keys and fallbacks.
//!
//! Lang files are java.util.Properties in UTF-8 directly under `shaders/lang/`; their file
//! names compare without regard to case (`en_US.lang` is `en_us`). Iris looks up
//! `option.<NAME>`, `option.<NAME>.comment`, `value.<NAME>.<value>`, `prefix.<NAME>`,
//! `suffix.<NAME>`, `screen.<ID>`, `screen.<ID>.comment`, `profile.<NAME>` and
//! `profile.comment`, falling back to the bare name or value. Text keeps the Minecraft
//! formatting codes Java Edition and Bedrock draw alike; see [`formatted`].

use std::collections::HashMap;

use crate::properties::reader::read_plain_entries;
use crate::source::PackSource;

/// The language Iris falls back to, and the one Cinnabar shows.
pub const DEFAULT_LANGUAGE: &str = "en_us";

/// One language's entries of a pack.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PackLang {
    entries: HashMap<String, String>,
}

impl PackLang {
    /// Reads Properties-format lang text; a repeated key keeps its last value.
    pub fn parse(text: &str) -> PackLang {
        PackLang {
            entries: read_plain_entries(text)
                .into_iter()
                .map(|entry| (entry.key, entry.value))
                .collect(),
        }
    }

    /// The pack's `lang/<code>.lang` (file name compared ignoring case); empty when absent.
    pub(crate) fn from_source(source: &PackSource, code: &str) -> PackLang {
        let file = format!("{code}.lang");
        source
            .files()
            .find(|(path, _)| {
                path.strip_prefix("lang/")
                    .is_some_and(|name| name.eq_ignore_ascii_case(&file))
            })
            .map_or_else(PackLang::default, |(_, text)| PackLang::parse(text))
    }

    /// The raw entry for `key`.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    /// `option.<NAME>`, else the option name.
    pub fn option_label(&self, name: &str) -> String {
        self.text(&format!("option.{name}"))
            .unwrap_or_else(|| name.to_owned())
    }

    /// `option.<NAME>.comment`, the option's tooltip.
    pub fn option_comment(&self, name: &str) -> Option<String> {
        self.text(&format!("option.{name}.comment"))
    }

    /// How a value option shows `value`: `prefix.<NAME>`, then `value.<NAME>.<value>` or else
    /// `value` followed by `suffix.<NAME>`.
    pub fn value_label(&self, name: &str, value: &str) -> String {
        let prefix = self.text(&format!("prefix.{name}")).unwrap_or_default();
        let shown = self
            .text(&format!("value.{name}.{value}"))
            .unwrap_or_else(|| {
                let suffix = self.text(&format!("suffix.{name}")).unwrap_or_default();
                format!("{}{suffix}", formatted(value))
            });
        prefix + &shown
    }

    /// `screen.<ID>`, else the screen id.
    pub fn screen_label(&self, id: &str) -> String {
        self.text(&format!("screen.{id}"))
            .unwrap_or_else(|| id.to_owned())
    }

    /// `screen.<ID>.comment`, the link's tooltip.
    pub fn screen_comment(&self, id: &str) -> Option<String> {
        self.text(&format!("screen.{id}.comment"))
    }

    /// `profile.<NAME>`, else the profile name.
    pub fn profile_label(&self, name: &str) -> String {
        self.text(&format!("profile.{name}"))
            .unwrap_or_else(|| name.to_owned())
    }

    /// `profile.comment`, the profile selector's tooltip.
    pub fn profile_comment(&self) -> Option<String> {
        self.text("profile.comment")
    }

    /// The entry for `key` with its formatting made drawable.
    fn text(&self, key: &str) -> Option<String> {
        self.get(key).map(formatted)
    }
}

/// Java formatting codes Bedrock text draws the same way: the sixteen colours (`0`–`9`,
/// `a`–`f`), obfuscated `k`, bold `l`, italic `o` and reset `r`.
const SHARED_FORMAT_CODES: &str = "0123456789abcdefklor";

/// `text` with every `§` code that Java Edition would not draw the same way in Bedrock text
/// removed: Java's strikethrough `m` and underline `n` are Bedrock colours, and Bedrock-only
/// colour codes mean nothing in Java. Java ignores unknown codes, so they vanish too; a
/// trailing lone `§` is dropped as Java drops it.
pub(crate) fn formatted(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '§' {
            out.push(c);
            continue;
        }
        if let Some(code) = chars.next()
            && SHARED_FORMAT_CODES.contains(code.to_ascii_lowercase())
        {
            out.push('§');
            out.push(code.to_ascii_lowercase());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_fall_back_to_names_and_values() {
        let lang = PackLang::parse(
            "option.GLOW = Glow\noption.GLOW.comment=Makes things glow.\nscreen.SKY=Sky\n\
             value.QUALITY.2=High\nprefix.QUALITY=Q \nsuffix.QUALITY=x\nprofile.LOW=Low\n\
             profile.comment=Presets.\n",
        );
        assert_eq!(lang.option_label("GLOW"), "Glow");
        assert_eq!(lang.option_label("MISSING"), "MISSING");
        assert_eq!(
            lang.option_comment("GLOW").as_deref(),
            Some("Makes things glow.")
        );
        assert_eq!(lang.option_comment("MISSING"), None);
        assert_eq!(lang.value_label("QUALITY", "2"), "Q High");
        assert_eq!(lang.value_label("QUALITY", "3"), "Q 3x");
        assert_eq!(lang.value_label("OTHER", "1.5"), "1.5");
        assert_eq!(lang.screen_label("SKY"), "Sky");
        assert_eq!(lang.screen_label("FOG"), "FOG");
        assert_eq!(lang.screen_comment("SKY"), None);
        assert_eq!(lang.profile_label("LOW"), "Low");
        assert_eq!(lang.profile_label("HIGH"), "HIGH");
        assert_eq!(lang.profile_comment().as_deref(), Some("Presets."));
    }

    #[test]
    fn lang_files_are_found_ignoring_case_and_read_as_properties() {
        let source = PackSource::from_files([
            (
                "lang/en_US.lang",
                "# comment\n#if X\noption.A=One \\\n  line\n",
            ),
            ("lang/de_de.lang", "option.A=Eins\n"),
            ("lang/nested/en_us.lang", "option.A=Nested\n"),
        ]);
        let lang = PackLang::from_source(&source, DEFAULT_LANGUAGE);
        assert_eq!(lang.option_label("A"), "One line");
        assert_eq!(lang.get("#if X"), None);
        assert_eq!(
            PackLang::from_source(&PackSource::from_files([("a.fsh", "")]), DEFAULT_LANGUAGE),
            PackLang::default()
        );
    }

    #[test]
    fn property_values_keep_trailing_escaped_and_nonbreaking_whitespace() {
        let lang = PackLang::parse(
            "prefix.QUALITY=Q \nsuffix.QUALITY=\u{a0}x \noption.A=\u{a0}name\\ \noption.B=last\\n\n",
        );
        assert_eq!(lang.value_label("QUALITY", "2"), "Q 2\u{a0}x ");
        assert_eq!(lang.option_label("A"), "\u{a0}name ");
        assert_eq!(lang.option_label("B"), "last\n");
    }

    #[test]
    fn formatting_keeps_codes_both_editions_draw_alike() {
        assert_eq!(formatted("§eLow§r ok"), "§eLow§r ok");
        assert_eq!(formatted("§AGreen"), "§aGreen");
        assert_eq!(formatted("§nunder§mstrike§gcoin"), "understrikecoin");
        assert_eq!(formatted("§lbold§oitalic§kx"), "§lbold§oitalic§kx");
        assert_eq!(formatted("end§"), "end");
    }
}

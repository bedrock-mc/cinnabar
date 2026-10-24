//! The player's option values for one pack, stored as Iris stores them: `<pack>.txt` beside
//! the pack in the shader-pack folder (`BSL.zip` → `BSL.zip.txt`), a java.util.Properties
//! file in ISO-8859-1 holding only the values that differ from the pack's defaults. Files
//! written here load in Java Iris and the other way round.

use std::io;
use std::path::{Path, PathBuf};

use crate::options::{OptionValues, PackOption, normalize_values};
use crate::properties::reader::read_plain_entries;

/// Where the values of the pack at `pack` (the zip or folder in the shader-pack folder) are
/// stored; `None` for a path without a file name.
pub fn option_file(pack: &Path) -> Option<PathBuf> {
    let mut name = pack.file_name()?.to_owned();
    name.push(".txt");
    Some(pack.with_file_name(name))
}

/// Reads saved values; a missing file holds none. Values are not checked against the pack.
pub fn read_option_file(path: &Path) -> io::Result<OptionValues> {
    match crate::source::read_file_bounded(path, crate::source::MAX_TEXT_FILE_BYTES) {
        Ok(bytes) => Ok(parse_option_file(&bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(OptionValues::new()),
        Err(error) => Err(error),
    }
}

/// Decodes option-file bytes as ISO-8859-1 Properties; a repeated key keeps its last value.
pub fn parse_option_file(bytes: &[u8]) -> OptionValues {
    let text: String = bytes.iter().map(|&byte| char::from(byte)).collect();
    read_plain_entries(&text)
        .into_iter()
        .map(|entry| (entry.key, entry.value))
        .collect()
}

/// Stores the values that differ from the pack's defaults (see [`normalize_values`]), or
/// removes the file when none do, as Iris does after every load and apply.
pub fn write_option_file(
    path: &Path,
    options: &[PackOption],
    values: &OptionValues,
) -> io::Result<()> {
    let values = normalize_values(options, values);
    if values.is_empty() {
        return match std::fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    }
    assets::write_blob_atomic(path, format_option_file(&values).as_bytes())
        .map_err(io::Error::other)
}

/// Encodes values as `Properties.store` does, minus its timestamp comment: one `key=value`
/// line each, sorted by key, with Properties escapes and `\uXXXX` for everything outside
/// printable ASCII.
pub fn format_option_file(values: &OptionValues) -> String {
    let mut out = String::new();
    for (key, value) in values {
        escape(key, true, &mut out);
        out.push('=');
        escape(value, false, &mut out);
        out.push('\n');
    }
    out
}

/// Appends `text` with `Properties.saveConvert` escaping: every space in a key, a value's
/// leading space, control characters, `\ = : # !` and non-ASCII characters.
fn escape(text: &str, key: bool, out: &mut String) {
    for (index, c) in text.chars().enumerate() {
        match c {
            ' ' if key || index == 0 => out.push_str("\\ "),
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{c}' => out.push_str("\\f"),
            '=' | ':' | '#' | '!' => {
                out.push('\\');
                out.push(c);
            }
            ' '..='~' => out.push(c),
            _ => {
                let mut units = [0; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04X}"));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::OptionKind;

    #[test]
    fn oversized_saved_options_fail_before_parsing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Pack.zip.txt");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(crate::source::MAX_TEXT_FILE_BYTES as u64 + 1)
            .unwrap();
        let Err(error) = read_option_file(&path) else {
            panic!("oversized options must fail");
        };
        assert_eq!(error.kind(), io::ErrorKind::FileTooLarge);
    }

    fn option(name: &str, kind: OptionKind) -> PackOption {
        PackOption {
            name: name.to_owned(),
            kind,
            file: "a.glsl".to_owned(),
            line: 1,
        }
    }

    /// GLOW (switch, off), QUALITY (value, 2 of 1–3), SUN (const float).
    fn options() -> Vec<PackOption> {
        vec![
            option("GLOW", OptionKind::Bool { default: false }),
            option(
                "QUALITY",
                OptionKind::Value {
                    default: "2".to_owned(),
                    allowed: vec!["1".to_owned(), "2".to_owned(), "3".to_owned()],
                },
            ),
            option(
                "sunPathRotation",
                OptionKind::Const {
                    ty: "float".to_owned(),
                    default: "0.0".to_owned(),
                    allowed: vec!["0.0".to_owned(), "30.0".to_owned()],
                },
            ),
        ]
    }

    fn values(pairs: &[(&str, &str)]) -> OptionValues {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn plain_properties_round_trip_significant_whitespace() {
        let values = OptionValues::from([
            (" key ".to_owned(), " value ".to_owned()),
            ("line".to_owned(), "last\n\t".to_owned()),
            ("nonbreaking".to_owned(), "\u{a0}label\u{a0}".to_owned()),
        ]);
        assert_eq!(
            parse_option_file(format_option_file(&values).as_bytes()),
            values
        );
    }

    #[test]
    fn the_file_sits_beside_the_pack_named_after_it() {
        assert_eq!(
            option_file(Path::new("packs/BSL_v10.zip")),
            Some(PathBuf::from("packs/BSL_v10.zip.txt"))
        );
        assert_eq!(
            option_file(Path::new("packs/Photon")),
            Some(PathBuf::from("packs/Photon.txt"))
        );
    }

    #[test]
    fn reads_java_properties_written_by_iris() {
        // Properties.store output: a timestamp comment, then escaped pairs.
        let text = b"#Mon Oct 19 12:00:00 UTC 2026\r\nGLOW=true\r\nNAME=a\\=b\\:c\\ d\r\n\
                     SUFFIX=\\u00B3\r\nLATIN=\xb3\r\n";
        assert_eq!(
            parse_option_file(text),
            values(&[
                ("GLOW", "true"),
                ("NAME", "a=b:c d"),
                ("SUFFIX", "\u{b3}"),
                ("LATIN", "\u{b3}"),
            ])
        );
    }

    #[test]
    fn writes_only_values_that_differ_from_defaults_and_reads_them_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Pack.zip.txt");
        let chosen = values(&[
            ("GLOW", "true"),
            ("QUALITY", "2"),
            ("sunPathRotation", "30.0"),
            ("UNKNOWN", "1"),
        ]);
        write_option_file(&path, &options(), &chosen).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "GLOW=true\nsunPathRotation=30.0\n"
        );
        assert_eq!(
            read_option_file(&path).unwrap(),
            values(&[("GLOW", "true"), ("sunPathRotation", "30.0")])
        );
        write_option_file(&path, &options(), &values(&[("GLOW", "false")])).unwrap();
        assert!(!path.exists(), "only defaults left removes the file");
        assert_eq!(read_option_file(&path).unwrap(), OptionValues::new());
        write_option_file(&path, &options(), &OptionValues::new()).unwrap();
    }

    #[test]
    fn escapes_like_properties_store() {
        let written = format_option_file(&values(&[("A B", " x=y #z"), ("C", "\u{b3}\u{1F600}")]));
        assert_eq!(written, "A\\ B=\\ x\\=y \\#z\nC=\\u00B3\\uD83D\\uDE00\n");
        assert_eq!(
            parse_option_file(written.as_bytes()),
            values(&[("A B", " x=y #z"), ("C", "\u{b3}\u{1F600}")])
        );
    }
    #[test]
    fn option_replacement_preserves_the_previous_file_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Pack.zip.txt");
        let previous = dir.path().join("previous.txt");
        std::fs::write(&path, b"QUALITY=1\n").unwrap();
        std::fs::hard_link(&path, &previous).unwrap();
        write_option_file(&path, &options(), &values(&[("GLOW", "true")])).unwrap();
        assert_eq!(std::fs::read(&previous).unwrap(), b"QUALITY=1\n");
        assert_eq!(
            read_option_file(&path).unwrap(),
            values(&[("GLOW", "true")])
        );
    }
}

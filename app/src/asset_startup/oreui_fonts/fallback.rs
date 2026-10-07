//! Shared installed Unicode fallback fonts; only missing characters are rasterized.

use assets::{FontGlyphRequests, RuntimeFontCatalog};
use pack_compiler::compile_native_fallback_fonts;
use render_model::{MAX_UI_FALLBACK_FONT_PAGES, UI_FALLBACK_FONT_PAGE_SIDE};
use std::{collections::BTreeSet, path::Path, sync::Arc};

mod cache;

const SOURCES: &[(&str, &str)] = &[
    ("NotoSansMerged-Regular-", ".ttf"),
    ("NotoSansSC-Regular-", ".otf"),
    ("NotoSansTC-Regular-", ".otf"),
    ("NotoSansJP-Regular-", ".otf"),
    ("NotoSansKR-Regular-", ".otf"),
    ("NotoSansArabic-Regular-", ".ttf"),
    ("NotoSansMongolian-Regular-", ".ttf"),
    ("NotoSansSyriac-Regular-", ".ttf"),
    ("NotoSansTamilSupplement-Regular-", ".ttf"),
];

struct Source {
    index: usize,
    bytes: Vec<u8>,
}

const MAX_CACHED_GLYPHS: usize = 2048;

fn priority(locale: &str) -> [usize; 9] {
    match locale {
        "ja" => [3, 2, 1, 4, 0, 5, 6, 7, 8],
        "ko" => [4, 2, 1, 3, 0, 5, 6, 7, 8],
        "zh_TW" => [2, 1, 3, 4, 0, 5, 6, 7, 8],
        "zh_CN" => [1, 2, 3, 4, 0, 5, 6, 7, 8],
        "ar" => [5, 0, 4, 2, 1, 3, 6, 7, 8],
        _ => [0, 1, 2, 3, 4, 5, 6, 7, 8],
    }
}

fn compile(
    sources: &[Source],
    characters: &[char],
    locale: &str,
) -> Result<RuntimeFontCatalog, String> {
    let ordered: Vec<_> = priority(locale)
        .into_iter()
        .filter_map(|index| sources.iter().find(|source| source.index == index))
        .map(|source| source.bytes.as_slice())
        .collect();
    compile_native_fallback_fonts(
        &ordered,
        characters,
        UI_FALLBACK_FONT_PAGE_SIDE,
        MAX_UI_FALLBACK_FONT_PAGES,
    )
    .map_err(|error| error.to_string())
}

pub(super) fn install(
    base: RuntimeFontCatalog,
    bundle: &Path,
    resource_root: &Path,
) -> Result<RuntimeFontCatalog, String> {
    let directory = bundle.join("fonts");
    let paths: Vec<_> = std::fs::read_dir(&directory)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    let mut sources = Vec::new();
    for (index, &(prefix, suffix)) in SOURCES.iter().enumerate() {
        let Some(path) = paths.iter().find(|path| {
            path.file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|name| {
                    name.strip_prefix(prefix)
                        .and_then(|s| s.strip_suffix(suffix))
                        .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit()))
                })
        }) else {
            continue;
        };
        let bytes = super::read_source(path, 12 * 1024 * 1024)?;
        sources.push(Source { index, bytes });
    }
    let choices =
        launcher::menu::settings_options::SettingsOptions::language_choices(resource_root);
    let mut warm: BTreeSet<char> = choices
        .iter()
        .flat_map(|(_, name)| name.chars())
        .filter(|&ch| {
            base.named_fonts()
                .values()
                .any(|font| font.glyph(ch).is_none())
        })
        .collect();
    warm.extend(
        (0x0250..=0x02af)
            .chain(0x1d00..=0x1d7f)
            .chain(0x2600..=0x26ff)
            .filter_map(char::from_u32),
    );
    let characters: Vec<_> = warm.into_iter().take(MAX_CACHED_GLYPHS / 2).collect();
    let initial = compile(&sources, &characters, "")?;
    let requests = Arc::new(FontGlyphRequests::default());
    requests.seed(characters.iter().copied());
    let mut cache = cache::GlyphCache::new(
        initial
            .glyphs()
            .iter()
            .map(|glyph| glyph.codepoint)
            .collect(),
        MAX_CACHED_GLYPHS,
    );
    let combined = base
        .with_shared_fallback(&initial, Arc::clone(&requests))
        .map_err(|e| e.to_string())?;
    std::thread::Builder::new()
        .name("oreui-font-fallback".into())
        .spawn(move || {
            while Arc::strong_count(&requests) > 1 {
                let Some((locale, pending)) = requests.wait() else {
                    continue;
                };
                let update = cache.update(pending, |characters| {
                    compile(&sources, characters, &locale).map(|font| {
                        let supported = font.glyphs().iter().map(|glyph| glyph.codepoint).collect();
                        (font, supported)
                    })
                });
                requests.forget(update.evicted);
                if let Some(font) = update.font {
                    requests.publish(Arc::new(font));
                }
                if let Some(reason) = update.error {
                    eprintln!("OreUI fallback unavailable ({reason})");
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(combined)
}

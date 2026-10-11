//! HTTP(S) targets and clipped glyph hits from the rendered chat history.
use super::{
    super::TextMetrics,
    engine::{label_origin, painted_label_request},
};
use assets::RuntimeFontCatalog;
use json_ui::{Draw, DrawNode, ViewState};
use std::ops::Range;
use ui::{TextLayoutCache, UiPoint, UiRect};

struct Target {
    chars: Range<usize>,
    url: String,
}

fn targets(text: &str) -> Vec<Target> {
    let Ok(spans) = ui::parse_bedrock_text(text, ui::UiLimits::MAX_TEXT_BYTES) else {
        return Vec::new();
    };
    let plain = spans.plain_text();
    let mut found = Vec::new();
    for (start, _) in plain.char_indices().filter(|(_, c)| !c.is_whitespace()) {
        if start > 0
            && plain[..start]
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_whitespace() && !matches!(c, '(' | '[' | '<' | '"' | '\''))
        {
            continue;
        }
        let end = plain[start..]
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\''))
            .map_or(plain.len(), |n| start + n);
        let mut candidate = &plain[start..end];
        candidate = candidate.trim_end_matches(['.', ',', ';', ':', '!', '?']);
        while candidate.ends_with(')')
            && candidate.matches(')').count() > candidate.matches('(').count()
        {
            candidate = &candidate[..candidate.len() - 1];
        }
        while candidate.ends_with(']')
            && candidate.matches(']').count() > candidate.matches('[').count()
        {
            candidate = &candidate[..candidate.len() - 1];
        }
        let explicit = candidate
            .get(..7)
            .is_some_and(|p| p.eq_ignore_ascii_case("http://"))
            || candidate
                .get(..8)
                .is_some_and(|p| p.eq_ignore_ascii_case("https://"));
        let authority = candidate.split('/').next().unwrap_or_default();
        let bare = !candidate.contains('@')
            && !candidate.contains(':')
            && authority.split('.').count() >= 2
            && authority.split('.').all(|part| {
                !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            })
            && authority
                .rsplit('.')
                .next()
                .is_some_and(|tld| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()));
        if !explicit && !bare {
            continue;
        }
        let url = if explicit {
            candidate.to_owned()
        } else {
            format!("https://{candidate}")
        };
        let Ok(parsed) = url::Url::parse(&url) else {
            continue;
        };
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            continue;
        }
        found.push(Target {
            chars: plain[..start].chars().count()..plain[..start + candidate.len()].chars().count(),
            url,
        });
    }
    found
}

#[allow(clippy::too_many_arguments)]
pub(super) fn hits(
    nodes: &[DrawNode],
    view: &ViewState,
    metrics: TextMetrics,
    font: &RuntimeFontCatalog,
    layouts: &mut TextLayoutCache,
    scale: f32,
    origin: [f32; 2],
) -> Vec<(String, UiRect, String)> {
    let mut result = Vec::new();
    for node in nodes {
        if node.alpha <= 0.0
            || !node.shown(view)
            || !(node.key.contains("messages_text")
                || node.key.contains("messages_factory")
                || node.key.contains("messages_panel"))
        {
            continue;
        }
        let Draw::Text {
            text,
            scale: factor,
            align,
            options,
            ..
        } = &node.draw
        else {
            continue;
        };
        let targets = targets(text);
        if targets.is_empty() {
            continue;
        }
        let dest = [
            node.dest.x as f32 * scale,
            node.dest.y as f32 * scale,
            (node.dest.x + node.dest.w) as f32 * scale,
            (node.dest.y + node.dest.h) as f32 * scale,
        ];
        let request =
            painted_label_request(metrics, text, dest, font, *factor, options, *align, scale);
        let paint_origin = label_origin(dest, font, options, scale);
        let Ok(layout) = layouts.layout(request) else {
            continue;
        };
        for target in targets {
            let mut lines = std::collections::BTreeMap::<u16, [f32; 4]>::new();
            for (glyph, source) in layout.glyphs().iter().zip(layout.glyph_source_indices()) {
                if !source.is_some_and(|source| target.chars.contains(&source)) {
                    continue;
                }
                let [x0, y0, x1, y1] = glyph.bounds_64.map(|n| n as f32 / 64.0);
                let bounds = [
                    paint_origin[0] + x0,
                    paint_origin[1] + y0,
                    paint_origin[0] + x1,
                    paint_origin[1] + y1,
                ];
                lines
                    .entry(glyph.line)
                    .and_modify(|rect| {
                        rect[0] = rect[0].min(bounds[0]);
                        rect[1] = rect[1].min(bounds[1]);
                        rect[2] = rect[2].max(bounds[2]);
                        rect[3] = rect[3].max(bounds[3]);
                    })
                    .or_insert(bounds);
            }
            for bounds in lines.into_values() {
                let clip = [
                    node.clip.x as f32 * scale,
                    node.clip.y as f32 * scale,
                    (node.clip.x + node.clip.w) as f32 * scale,
                    (node.clip.y + node.clip.h) as f32 * scale,
                ];
                let [x0, y0, x1, y1] = [
                    bounds[0].max(clip[0]),
                    bounds[1].max(clip[1]),
                    bounds[2].min(clip[2]),
                    bounds[3].min(clip[3]),
                ];
                if x1 <= x0 || y1 <= y0 {
                    continue;
                }
                if let (Ok(min), Ok(max)) = (
                    UiPoint::new(x0 + origin[0], y0 + origin[1]),
                    UiPoint::new(x1 + origin[0], y1 + origin[1]),
                ) && let Ok(rect) = UiRect::new(min, max)
                {
                    result.push((target.url.clone(), rect, node.key.clone()));
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    mod hud_rows;

    #[test]
    fn web_targets_keep_punctuation_balanced_and_strip_formatting() {
        let links = targets(
            "Go (§bhttps://example.org/a_(b)§r), www.example.com, example.net/test! ftp://bad.test file:///tmp user@example.com javascript:example.com",
        );
        assert_eq!(
            links.iter().map(|l| l.url.as_str()).collect::<Vec<_>>(),
            [
                "https://example.org/a_(b)",
                "https://www.example.com",
                "https://example.net/test"
            ]
        );
        assert!(targets("https://user:password@example.com http:/// https://").is_empty());
    }
    fn history_node(text: &str, width: f64) -> DrawNode {
        DrawNode {
            name: "text".into(),
            key: "chat/messages_panel/messages_text/text".into(),
            dest: json_ui::RectOut {
                x: 10.0,
                y: 20.0,
                w: width,
                h: 400.0,
            },
            clip: json_ui::RectOut {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 500.0,
            },
            layer: 1,
            alpha: 1.0,
            anim: None,
            gates: Vec::new(),
            draw: Draw::Text {
                text: text.into(),
                color: [255; 4],
                shadow: true,
                align: json_ui::TextAlign::Left,
                scale: 1.0,
                localize: false,
                options: Default::default(),
            },
        }
    }
    #[test]
    fn multiple_links_hit_their_own_glyphs_and_wrapped_links_keep_one_target() {
        let font = super::super::super::tests::fixture_font();
        let metrics =
            TextMetrics::for_viewport([800, 600], ui::DpiScale::new(1.0).unwrap(), Some(1));
        let scale = metrics.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32;
        let mut layouts = TextLayoutCache::new(32, 1 << 20);
        let node = history_node(
            "prefix https://one.example then https://two.example suffix",
            600.0,
        );
        let regions = hits(
            &[node],
            &ViewState::default(),
            metrics,
            &font,
            &mut layouts,
            scale,
            [3.0, 7.0],
        );
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].0, "https://one.example");
        assert_eq!(regions[1].0, "https://two.example");
        assert!(regions[0].1.max().x() < regions[1].1.min().x());
        let wrapped = history_node("https://example.com/a_very_long_path", 25.0);
        let regions = hits(
            std::slice::from_ref(&wrapped),
            &ViewState::default(),
            metrics,
            &font,
            &mut layouts,
            scale,
            [0.0; 2],
        );
        assert!(regions.len() > 1);
        assert!(
            regions
                .iter()
                .all(|r| r.0 == "https://example.com/a_very_long_path")
        );
        let Draw::Text {
            text,
            scale: factor,
            align,
            options,
            ..
        } = &wrapped.draw
        else {
            unreachable!()
        };
        let dest = [10.0 * scale, 20.0 * scale, 35.0 * scale, 420.0 * scale];
        let layout = layouts
            .layout(painted_label_request(
                metrics, text, dest, &font, *factor, options, *align, scale,
            ))
            .unwrap();
        let plain = ui::parse_bedrock_text(text, ui::UiLimits::MAX_TEXT_BYTES)
            .unwrap()
            .plain_text()
            .chars()
            .collect::<Vec<_>>();
        assert!(layout.glyph_source_indices().contains(&None));
        for (glyph, source) in layout.glyphs().iter().zip(layout.glyph_source_indices()) {
            if let Some(index) = source {
                assert_eq!(glyph.codepoint, plain[*index]);
            }
        }
    }

    #[test]
    fn link_hits_follow_the_actual_default_and_named_label_paint_origin() {
        use std::sync::Arc;

        use super::super::engine::{EngineInputs, EngineOutput, FormEngine, ScreenArt};
        use crate::ui_runtime::presentation::UiPresentationRuntime;

        for named in [false, true] {
            let base = crate::test_support::fixture_font();
            let font = if named {
                Arc::new(base.with_named_font("alternative", &base).unwrap())
            } else {
                base
            };
            for gui_scale in [1u8, 2] {
                let physical = [800, 600].map(|side| side * u32::from(gui_scale));
                let metrics = TextMetrics::for_viewport(
                    physical,
                    ui::DpiScale::new(1.0).unwrap(),
                    Some(gui_scale),
                );
                let px = metrics.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32;
                let mut presentation = UiPresentationRuntime::new(Arc::clone(&font)).unwrap();
                let engine =
                    FormEngine::new(crate::test_support::mini_carrier(), Default::default(), 0);
                let mut node = history_node("https://example.com", 600.0);
                if let Draw::Text { options, .. } = &mut node.draw {
                    options.font_type = named.then(|| "alternative".into());
                }
                let render = json_ui::FormRender {
                    bound: json_ui::ResolvedControl {
                        name: "fixture".into(),
                        control_type: Some("panel".into()),
                        base: None,
                        unresolved_base: None,
                        properties: Default::default(),
                        children: Vec::new(),
                        factory: None,
                    },
                    nodes: vec![node.clone()],
                    hits: Arc::from([]),
                    report: Default::default(),
                    cancel_target: None,
                    root_panel: None,
                };
                let mut painted = Vec::new();
                let mut next = 1;
                engine
                    .draw(
                        ScreenArt::default(),
                        EngineInputs {
                            layouts: &mut presentation.layouts,
                            font: &font,
                            metrics,
                            solid_page: presentation.solid_texture_page,
                            safe_area: ui::SafeArea::ZERO,
                            content: physical.map(|side| side as f32),
                            translate: &|_| None,
                            language: [0; 3],
                        },
                        EngineOutput {
                            nodes: &mut painted,
                            next: &mut next,
                            overlay: &[],
                        },
                        |_, _| Some(render),
                    )
                    .unwrap()
                    .expect("history label paints");
                let text = painted
                    .iter()
                    .find(|node| matches!(node.visual(), ui::UiVisual::Text { .. }))
                    .expect("retained text exists");
                let origin = text.bounds().min();
                assert_eq!(origin.x(), 10.0 * px);
                assert_eq!(origin.y(), (20.0 + if named { 0.0 } else { 1.0 }) * px);
                let ui::UiVisual::Text { layout, .. } = text.visual() else {
                    unreachable!()
                };
                let mut ink = [
                    f32::INFINITY,
                    f32::INFINITY,
                    f32::NEG_INFINITY,
                    f32::NEG_INFINITY,
                ];
                for glyph in layout.glyphs() {
                    let [x0, y0, x1, y1] = glyph.bounds_64.map(|edge| edge as f32 / 64.0);
                    ink = [
                        ink[0].min(x0),
                        ink[1].min(y0),
                        ink[2].max(x1),
                        ink[3].max(y1),
                    ];
                }
                let regions = hits(
                    &[node],
                    &ViewState::default(),
                    metrics,
                    &font,
                    &mut presentation.layouts,
                    px,
                    [0.0; 2],
                );
                assert_eq!(regions.len(), 1);
                let bounds = regions[0].1;
                assert_eq!(bounds.min().x(), origin.x() + ink[0]);
                assert_eq!(bounds.min().y(), origin.y() + ink[1]);
                assert_eq!(bounds.max().x(), origin.x() + ink[2]);
                assert_eq!(bounds.max().y(), origin.y() + ink[3]);
            }
        }
    }

    #[test]
    fn clipped_and_non_history_text_never_produce_background_link_hits() {
        let font = super::super::super::tests::fixture_font();
        let metrics =
            TextMetrics::for_viewport([800, 600], ui::DpiScale::new(1.0).unwrap(), Some(1));
        let scale = metrics.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32;
        let mut layouts = TextLayoutCache::new(32, 1 << 20);
        let mut node = history_node("https://example.com", 500.0);
        let original = hits(
            &[node.clone()],
            &ViewState::default(),
            metrics,
            &font,
            &mut layouts,
            scale,
            [0.0; 2],
        );
        assert_eq!(original.len(), 1);
        node.clip = json_ui::RectOut {
            x: 12.0,
            y: 20.0,
            w: 3.0,
            h: 400.0,
        };
        let clipped = hits(
            &[node.clone()],
            &ViewState::default(),
            metrics,
            &font,
            &mut layouts,
            scale,
            [0.0; 2],
        );
        assert_eq!(clipped.len(), 1);
        assert!(clipped[0].1.width() < original[0].1.width());
        assert!(clipped[0].1.min().x() >= 12.0 * scale);
        assert!(clipped[0].1.max().x() <= 15.0 * scale);
        node.clip.y = -200.0;
        node.clip.h = 1.0;
        assert!(
            hits(
                &[node.clone()],
                &ViewState::default(),
                metrics,
                &font,
                &mut layouts,
                scale,
                [0.0; 2]
            )
            .is_empty()
        );
        node = history_node("https://example.com", 500.0);
        node.key = "chat/text_edit_box/text".into();
        assert!(
            hits(
                &[node],
                &ViewState::default(),
                metrics,
                &font,
                &mut layouts,
                scale,
                [0.0; 2]
            )
            .is_empty()
        );
    }
}

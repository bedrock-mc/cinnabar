use std::{fmt, ops::Range, sync::Arc};

use assets::RuntimeFontCatalog;
use bevy::{
    prelude::{Query, Res, ResMut, Resource, Time, With},
    time::Real,
    window::{PrimaryWindow, Window},
};
use render::{
    MAX_UI_TEXTURE_BYTES, UiRenderInput, UiRenderScene, UiRenderStats, UiRenderTextureArray,
};
use ui::{
    DpiScale, HudViewRole, SafeArea, TextLayoutCache, TextLayoutRequest, TextStyle, UiNode,
    UiNodeId, UiPoint, UiRect, UiScale, UiTree, UiVisual,
};

use super::{UiRuntime, render_adapter::UiRenderViewport};
use crate::{
    runtime::{shutdown::record_fatal_error, world::ClientWorld},
    ui_runtime::render_adapter::adapt_ui_draw_list,
};

const TEXT_CACHE_ENTRIES: usize = 1_024;
const TEXT_CACHE_BYTES: usize = 8 * 1024 * 1024;
const MAX_PRESENTED_CHAT_ROWS: usize = 8;
const MAX_PRESENTED_CHAT_SUGGESTIONS: usize = 8;
const MAX_PRESENTED_TOAST_ROWS: usize = 8;
const MAX_PRESENTED_TEXT_BYTES: usize = 512;

#[derive(Debug)]
pub enum UiPresentationError {
    InvalidFontTexture,
    Geometry(ui::GeometryError),
    Text(ui::TextError),
    Tree(ui::UiError),
    Adapter(super::render_adapter::UiRenderAdapterError),
    Render(render::UiRenderReject),
}

impl fmt::Display for UiPresentationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "UI presentation failed: {self:?}")
    }
}

impl std::error::Error for UiPresentationError {}

#[derive(Resource)]
pub struct UiPresentationRuntime {
    font: Arc<RuntimeFontCatalog>,
    textures: Arc<UiRenderTextureArray>,
    layouts: TextLayoutCache,
    revision: u64,
}

impl UiPresentationRuntime {
    pub fn new(font: Arc<RuntimeFontCatalog>) -> Result<Self, UiPresentationError> {
        let textures = Arc::new(font_texture_array(&font)?);
        Ok(Self {
            font,
            textures,
            layouts: TextLayoutCache::new(TEXT_CACHE_ENTRIES, TEXT_CACHE_BYTES),
            revision: 0,
        })
    }

    pub fn build(
        &mut self,
        runtime: &UiRuntime,
        now_millis: u64,
        physical_size: [u32; 2],
        dpi_scale: DpiScale,
    ) -> Result<UiRenderInput, UiPresentationError> {
        let logical_width = physical_size[0] as f32 / dpi_scale.get();
        let logical_height = physical_size[1] as f32 / dpi_scale.get();
        let viewport = rect(0.0, 0.0, logical_width, logical_height)?;
        let wrap_width = ((logical_width * 0.45).clamp(1.0, 640.0) * 64.0) as u32;
        let mut nodes = Vec::new();
        let mut next_id = 1u32;

        let hud_nodes = runtime.hud().view_nodes(now_millis);
        let mut toast_rows = 0usize;
        for node in hud_nodes.iter() {
            if matches!(
                node.role,
                HudViewRole::ToastTitle | HudViewRole::ToastMessage
            ) {
                if toast_rows >= MAX_PRESENTED_TOAST_ROWS {
                    continue;
                }
                toast_rows += 1;
            }
            let text = bounded_visible_text(&node.text);
            let layout = self
                .layouts
                .layout(TextLayoutRequest {
                    text,
                    style: TextStyle::default(),
                    width_64: wrap_width,
                    scale: UiScale::default(),
                    font: &self.font,
                })
                .map_err(UiPresentationError::Text)?;
            let [x, y] = hud_position(node.role, nodes.len(), logical_width, logical_height);
            nodes.push(
                UiNode::new(
                    UiNodeId::new(next_id),
                    None,
                    rect(
                        x,
                        y,
                        (x + logical_width * 0.45).min(logical_width),
                        logical_height,
                    )?,
                )
                .with_visual(UiVisual::Text {
                    layout,
                    color: [255; 4],
                }),
            );
            next_id = next_id.saturating_add(1);
        }

        let chat = runtime.chat().view_nodes();
        let first = chat.len().saturating_sub(MAX_PRESENTED_CHAT_ROWS);
        for (row, node) in chat[first..].iter().enumerate() {
            let text = bounded_visible_text(&node.text);
            let layout = self
                .layouts
                .layout(TextLayoutRequest {
                    text,
                    style: TextStyle::default(),
                    width_64: wrap_width,
                    scale: UiScale::default(),
                    font: &self.font,
                })
                .map_err(UiPresentationError::Text)?;
            let y = (logical_height - 220.0 + row as f32 * 20.0).max(0.0);
            nodes.push(
                UiNode::new(
                    UiNodeId::new(next_id),
                    None,
                    rect(
                        12.0,
                        y,
                        (12.0 + logical_width * 0.45).min(logical_width),
                        logical_height,
                    )?,
                )
                .with_visual(UiVisual::Text {
                    layout,
                    color: [255; 4],
                }),
            );
            next_id = next_id.saturating_add(1);
        }

        if runtime.chat_focused() {
            let editor = runtime.chat_editor();
            let mut visible = String::with_capacity(editor.len_bytes().saturating_add(3));
            visible.push_str("> ");
            visible.push_str(&editor.as_str()[..editor.cursor_byte()]);
            visible.push('|');
            visible.push_str(&editor.as_str()[editor.cursor_byte()..]);
            let layout = self
                .layouts
                .layout(TextLayoutRequest {
                    text: bounded_visible_text(&visible),
                    style: TextStyle::default(),
                    width_64: wrap_width,
                    scale: UiScale::default(),
                    font: &self.font,
                })
                .map_err(UiPresentationError::Text)?;
            let editor_y = (logical_height - 40.0).max(0.0);
            nodes.push(
                UiNode::new(
                    UiNodeId::new(next_id),
                    None,
                    rect(
                        12.0,
                        editor_y,
                        (12.0 + logical_width * 0.45).min(logical_width),
                        logical_height,
                    )?,
                )
                .with_visual(UiVisual::Text {
                    layout,
                    color: [255; 4],
                }),
            );
            next_id = next_id.saturating_add(1);

            let visible = visible_suggestion_range(
                runtime.chat_suggestions().len(),
                runtime.chat_selected_suggestion(),
            );
            for (row, (index, suggestion)) in runtime
                .chat_suggestions()
                .iter()
                .enumerate()
                .skip(visible.start)
                .take(visible.len())
                .enumerate()
            {
                let selected = runtime.chat_selected_suggestion() == Some(index);
                let mut visible = String::with_capacity(suggestion.len().saturating_add(2));
                visible.push_str(if selected { "> " } else { "  " });
                visible.push_str(suggestion);
                let layout = self
                    .layouts
                    .layout(TextLayoutRequest {
                        text: bounded_visible_text(&visible),
                        style: TextStyle::default(),
                        width_64: wrap_width,
                        scale: UiScale::default(),
                        font: &self.font,
                    })
                    .map_err(UiPresentationError::Text)?;
                let bottom = (editor_y - (row as f32 + 1.0) * 18.0).max(0.0);
                nodes.push(
                    UiNode::new(
                        UiNodeId::new(next_id),
                        None,
                        rect(
                            12.0,
                            bottom,
                            (12.0 + logical_width * 0.45).min(logical_width),
                            editor_y,
                        )?,
                    )
                    .with_visual(UiVisual::Text {
                        layout,
                        color: [220, 220, 220, 255],
                    }),
                );
                next_id = next_id.saturating_add(1);
            }
        }

        let mut tree = UiTree::new(nodes).map_err(UiPresentationError::Tree)?;
        tree.layout(viewport, UiScale::default(), SafeArea::ZERO)
            .map_err(UiPresentationError::Tree)?;
        let mut draw_list = tree.build_draw_list().map_err(UiPresentationError::Tree)?;
        self.revision = self.revision.saturating_add(1);
        draw_list.revision = self.revision;
        adapt_ui_draw_list(
            &draw_list,
            Arc::clone(&self.textures),
            UiRenderViewport {
                physical_size,
                dpi_scale,
                safe_area: SafeArea::ZERO,
            },
        )
        .map_err(UiPresentationError::Adapter)
    }
}

pub(super) fn visible_suggestion_range(total: usize, selected: Option<usize>) -> Range<usize> {
    let selected = selected.unwrap_or(0).min(total.saturating_sub(1));
    let end = total.min(
        selected
            .saturating_add(1)
            .max(MAX_PRESENTED_CHAT_SUGGESTIONS),
    );
    end.saturating_sub(MAX_PRESENTED_CHAT_SUGGESTIONS)..end
}

pub(super) fn hit_test_chat_suggestion(
    position: ui::UiPoint,
    logical_size: [f32; 2],
    total: usize,
    selected: Option<usize>,
) -> Option<usize> {
    if position.x() < 12.0 || position.x() > 12.0 + logical_size[0] * 0.45 {
        return None;
    }
    let editor_y = (logical_size[1] - 40.0).max(0.0);
    let distance = editor_y - position.y();
    if !(0.0..MAX_PRESENTED_CHAT_SUGGESTIONS as f32 * 18.0).contains(&distance) {
        return None;
    }
    let visible_row = (distance / 18.0).floor() as usize;
    let visible = visible_suggestion_range(total, selected);
    visible
        .start
        .checked_add(visible_row)
        .filter(|index| *index < visible.end)
}

pub(crate) fn publish_ui_runtime(
    mut runtime: ResMut<UiRuntime>,
    mut presentation: ResMut<UiPresentationRuntime>,
    mut scene: ResMut<UiRenderScene>,
    stats: Res<UiRenderStats>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut client_world: ResMut<ClientWorld>,
    time: Res<Time<Real>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let physical_size = [window.physical_width(), window.physical_height()];
    if physical_size.contains(&0) {
        return;
    }
    let Ok(dpi_scale) = DpiScale::new(window.scale_factor()) else {
        record_fatal_error(
            &mut client_world.fatal_error,
            "primary window reported an unsupported UI DPI scale".to_owned(),
        );
        return;
    };
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    runtime.hud.expire(now_millis);
    let input = match presentation.build(&runtime, now_millis, physical_size, dpi_scale) {
        Ok(input) => input,
        Err(error) => {
            record_fatal_error(&mut client_world.fatal_error, error.to_string());
            return;
        }
    };
    if let Err(error) = scene.publish(input, &stats) {
        record_fatal_error(
            &mut client_world.fatal_error,
            UiPresentationError::Render(error).to_string(),
        );
    }
}

fn font_texture_array(
    font: &RuntimeFontCatalog,
) -> Result<UiRenderTextureArray, UiPresentationError> {
    let width = font
        .pages()
        .iter()
        .map(|page| page.width)
        .max()
        .ok_or(UiPresentationError::InvalidFontTexture)?;
    let height = font
        .pages()
        .iter()
        .map(|page| page.height)
        .max()
        .ok_or(UiPresentationError::InvalidFontTexture)?;
    let layers =
        u32::try_from(font.pages().len()).map_err(|_| UiPresentationError::InvalidFontTexture)?;
    let layer_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(height as usize))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(UiPresentationError::InvalidFontTexture)?;
    let total_bytes = layer_bytes
        .checked_mul(layers as usize)
        .filter(|bytes| *bytes <= MAX_UI_TEXTURE_BYTES)
        .ok_or(UiPresentationError::InvalidFontTexture)?;
    let mut rgba8 = vec![0; total_bytes];
    for (layer, page) in font.pages().iter().enumerate() {
        let page_width = page.width as usize;
        let page_height = page.height as usize;
        for row in 0..page_height {
            let source_start = row * page_width * 4;
            let source_end = source_start + page_width * 4;
            let target_start = layer * layer_bytes + row * width as usize * 4;
            rgba8[target_start..target_start + page_width * 4]
                .copy_from_slice(&page.rgba8[source_start..source_end]);
        }
    }
    Ok(UiRenderTextureArray {
        identity: font.identity().carrier_sha256,
        width,
        height,
        layers,
        rgba8: rgba8.into(),
    })
}

fn bounded_visible_text(value: &str) -> &str {
    if value.len() <= MAX_PRESENTED_TEXT_BYTES {
        return value;
    }
    let mut end = MAX_PRESENTED_TEXT_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn hud_position(role: HudViewRole, ordinal: usize, width: f32, height: f32) -> [f32; 2] {
    match role {
        HudViewRole::Health => [12.0, (height - 42.0).max(0.0)],
        HudViewRole::Hunger => [(width - 180.0).max(0.0), (height - 42.0).max(0.0)],
        HudViewRole::Armor => [12.0, (height - 62.0).max(0.0)],
        HudViewRole::Air => [(width - 180.0).max(0.0), (height - 62.0).max(0.0)],
        HudViewRole::Title => [(width * 0.3).max(0.0), (height * 0.3).max(0.0)],
        HudViewRole::Subtitle => [(width * 0.3).max(0.0), (height * 0.3 + 24.0).max(0.0)],
        HudViewRole::ActionBar => [(width * 0.35).max(0.0), (height - 90.0).max(0.0)],
        HudViewRole::ToastTitle | HudViewRole::ToastMessage => {
            [(width - 320.0).max(0.0), 12.0 + ordinal as f32 * 18.0]
        }
    }
}

fn rect(left: f32, top: f32, right: f32, bottom: f32) -> Result<UiRect, UiPresentationError> {
    UiRect::new(
        UiPoint::new(left, top).map_err(UiPresentationError::Geometry)?,
        UiPoint::new(right, bottom).map_err(UiPresentationError::Geometry)?,
    )
    .map_err(UiPresentationError::Geometry)
}

#[cfg(test)]
mod tests {
    use assets::{FontTexturePage, GlyphMetrics, RuntimeFontCatalog, encode_font_catalog};
    use protocol::{HudEvent, UiEvent};
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::ui_runtime::SequencedUiEvent;

    #[test]
    fn retained_hud_publishes_through_tree_adapter_and_render_scene() {
        let font = fixture_font();
        let mut presentation = UiPresentationRuntime::new(font).unwrap();
        let mut runtime = UiRuntime::new(1);
        runtime
            .apply(SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Hud(HudEvent::Health { health: 20 }),
            })
            .unwrap();

        let input = presentation
            .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
            .unwrap();
        assert!(!input.vertices.is_empty());
        assert!(!input.indices.is_empty());
        assert!(!input.batches.is_empty());
        let mut scene = UiRenderScene::default();
        scene.publish(input, &UiRenderStats::default()).unwrap();
        assert!(scene.input.is_some());
    }

    #[test]
    fn focused_chat_editor_history_and_suggestions_are_presented() {
        let font = fixture_font();
        let mut presentation = UiPresentationRuntime::new(font).unwrap();
        let mut runtime = UiRuntime::new(1);
        let empty = presentation
            .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
            .unwrap();
        runtime.open_chat();
        runtime.insert_chat_text("/g").unwrap();
        runtime.take_chat_autocomplete_request().unwrap();
        runtime
            .apply(SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::ChatAutocomplete(protocol::ChatAutocompleteEvent {
                    enum_name: Arc::from("commands"),
                    action: protocol::ChatAutocompleteAction::Replace,
                    suggestions: Arc::from([Arc::from("/give"), Arc::from("/gamemode")]),
                }),
            })
            .unwrap();

        let active = presentation
            .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
            .unwrap();

        assert!(active.vertices.len() > empty.vertices.len());
        assert!(active.indices.len() > empty.indices.len());
    }

    #[test]
    fn suggestion_window_keeps_the_selected_row_visible() {
        assert_eq!(visible_suggestion_range(20, Some(12)), 5..13);
        assert_eq!(visible_suggestion_range(20, Some(19)), 12..20);
        assert_eq!(visible_suggestion_range(3, Some(2)), 0..3);
    }

    fn fixture_font() -> Arc<RuntimeFontCatalog> {
        let pixels = vec![255; 4].into_boxed_slice();
        let page = FontTexturePage {
            source_path: "font/page.png".into(),
            source_bytes: 4,
            source_sha256: [1; 32],
            pixels_sha256: Sha256::digest(&pixels).into(),
            width: 1,
            height: 1,
            rgba8: pixels,
        };
        let glyphs = ['/', '0', '2', '\u{fffd}'].map(|codepoint| GlyphMetrics {
            codepoint,
            page: 0,
            uv: [0, 0, 1, 1],
            bearing: [0, 0],
            advance_64: 64,
        });
        let manifest = [7; 32];
        let bytes = encode_font_catalog(manifest, &glyphs, &[page]).unwrap();
        Arc::new(RuntimeFontCatalog::decode(&bytes, manifest).unwrap())
    }
}

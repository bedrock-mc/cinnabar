//! Production screen painting into the JSON-UI editor's existing CPU target.

use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, LazyLock},
};

use image::ImageEncoder;
use jsonui_editor::raster::{Canvas, Page};
use oreui::menu_screen::{MenuGeometry, MenuInput};
use ui::{DpiScale, SafeArea, TextLayoutCache, UiScale, UiTree};
use ui_engine::{
    engine::{
        FormEngine,
        screen::{PaintFrame, Screen, ScreenRegistry},
    },
    sources::{ImageWork, RemoteImage, RemoteSource},
    text_metrics::TextMetrics,
};
use ui_screens::chat_screen::{ChatInput, ChatScreen};

use crate::{
    carriers::{self, Error},
    fixtures::{Fixture, State},
};

pub const CAPTURE_SECONDS: f64 = 1.0;

static REGISTRY: LazyLock<ScreenRegistry> = LazyLock::new(|| {
    let mut registry = ScreenRegistry::new(ui_screens::menu_screens::retail_context());
    registry.screen(ui_screens::chat_screen::CHAT_SCREEN);
    ui_screens::hud::register(&mut registry);
    ui_screens::chat_screen::register(&mut registry);
    registry
});

struct Offline;

impl RemoteSource for Offline {
    /// Snapshot fixtures never download images or start network work.
    fn state(&self, _: &str) -> RemoteImage {
        RemoteImage::Failed
    }
}

/// Paints a named state at fixed times and encodes deterministic RGBA PNG bytes.
pub fn png(state: &State, dir: &Path) -> Result<Vec<u8>, Error> {
    let fixture = (state.fixture)();
    let menu = matches!(&fixture, Fixture::Menu(_));
    let mut carriers = carriers::load(dir, menu)?;
    let size = state.size.map(|value| value as f32);
    let metrics = TextMetrics::for_viewport(
        state.size,
        DpiScale::new(1.0).unwrap(),
        Some(state.gui_scale),
    );
    let mut nodes = Vec::new();
    let mut next = 1;
    let mut layouts = TextLayoutCache::new(4096, 32 * 1024 * 1024);
    let translate = |key: &str| carriers.lang.lookup(key);
    let mut frame = PaintFrame {
        layouts: &mut layouts,
        font: &carriers.font,
        metrics,
        solid_page: carriers.solid_page,
        safe_area: SafeArea::ZERO,
        content: size,
        translate: &translate,
        language: [0; 3],
        now: 0.0,
        nodes: &mut nodes,
        next: &mut next,
        renderers: &mut [],
    };
    match fixture {
        Fixture::Menu(view) => {
            carriers::validate_oreui(dir)?;
            let images = oreui::images::load_optional_oreui_images(dir)
                .ok_or_else(|| Error::Invalid("OreUI artwork is unusable; make assets".into()))?;
            let first = carriers.pages.len() as u16;
            let artwork: HashMap<_, _> = images
                .sprites
                .iter()
                .map(|(key, sprite)| {
                    (
                        key.clone(),
                        ui::IconRef {
                            page: first + sprite.page,
                            uv: sprite.bounds,
                            glint: false,
                        },
                    )
                })
                .collect();
            carriers.pages.extend(images.pages.iter().map(|page| Page {
                width: page.dimensions[0],
                height: page.dimensions[1],
                rgba: Arc::clone(&page.pixels),
            }));
            let mut screen = oreui_screen(first, images);
            let mut scrolls = ui_engine::menu_scroll::MenuScrolls::default();
            // Mount at zero, then sample settled motion without reading a clock or sleeping.
            for now in [0.0, CAPTURE_SECONDS] {
                frame.nodes.clear();
                *frame.next = 1;
                frame.now = now;
                screen
                    .paint(
                        MenuInput {
                            view: &view,
                            size,
                            portrait: None,
                            translate: frame.translate,
                            artwork: &artwork,
                            title_artwork: None,
                            scrolls: &mut scrolls,
                            geometry: MenuGeometry::default(),
                        },
                        &mut frame,
                    )
                    .map_err(|error| error.to_string())?;
                screen
                    .take_paint()
                    .unwrap()
                    .apply_entrance(frame.nodes, size)
                    .map_err(|error| error.to_string())?;
                if view.dialog == Some(launcher::menu::MenuDialog::MinecraftImport) {
                    screen
                        .paint_minecraft_import(&view, size, &mut scrolls, &mut frame)
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        Fixture::Chat { draft, suggestions } => {
            let catalog = json_ui::Catalog::from_files(
                carriers
                    .ui
                    .ui_files()
                    .iter()
                    .map(|file| (&*file.path, &*file.bytes)),
            )
            .map_err(|error| error.to_string())?;
            let mut engine = FormEngine::new(
                Arc::clone(&carriers.ui),
                catalog,
                carriers.ui_page,
                &REGISTRY,
                Arc::new(Offline),
                ImageWork::inline(),
            );
            engine.set_server_atlas(
                ui_engine::atlas::ServerAtlas::new(&[], None, ui_engine::pages::SERVER_UI_PAGES)
                    .with_image_work(ImageWork::inline()),
                carriers.pages.len() as u16,
            );
            let mut editor = ui::ChatEditor::new(ui::MAX_CHAT_INPUT_BYTES)
                .map_err(|error| format!("{error:?}"))?;
            editor.set_text(draft);
            frame.now = CAPTURE_SECONDS;
            let mut screen = ChatScreen::default();
            for now in [0.0, CAPTURE_SECONDS] {
                frame.nodes.clear();
                *frame.next = 1;
                frame.now = now;
                screen
                    .paint(
                        &engine,
                        ChatInput {
                            editor: &editor,
                            messages: &Default::default(),
                            suggestions: &suggestions,
                            usage: None,
                            selected_suggestion: Some(0),
                            annotations: &[],
                            clocks: &Default::default(),
                            now_millis: (now * 1000.0) as u64,
                        },
                        &mut frame,
                    )
                    .map_err(|error| error.to_string())?;
            }
            if screen.nodes().is_empty() {
                return Err(Error::Invalid("native chat screen did not resolve".into()));
            }
            if let Some(pages) = engine.take_server_pages() {
                // The inline image work finishes before painting; page order comes from the atlas.
                carriers.pages.extend(pages.iter().map(|page| {
                    let [width, height] = page.dimensions();
                    Page {
                        width,
                        height,
                        rgba: page.pixels().into(),
                    }
                }));
            }
        }
    }
    if nodes.is_empty() {
        return Err(Error::Invalid("screen painted no nodes".into()));
    }
    let mut tree = UiTree::new(nodes).map_err(|error| error.to_string())?;
    let viewport =
        ui_engine::engine::rect(0.0, 0.0, size[0], size[1]).map_err(|error| error.to_string())?;
    tree.layout(viewport, UiScale::default(), SafeArea::ZERO)
        .map_err(|error| error.to_string())?;
    let list = tree.build_draw_list().map_err(|error| error.to_string())?;
    if list.batches.iter().any(|batch| {
        carriers
            .pages
            .get(usize::from(batch.texture_page))
            .is_none()
    }) {
        return Err(Error::Invalid(
            "screen sampled an unavailable texture page".into(),
        ));
    }
    let mut canvas = Canvas::new(state.size[0], state.size[1]);
    canvas.draw(&list, |page| carriers.pages.get(usize::from(page)));
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            &canvas.into_rgba(),
            state.size[0],
            state.size[1],
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| error.to_string())?;
    Ok(png)
}

/// Attaches the atlas to the sprite and nine-slice painters with the client's artwork look.
pub(crate) fn oreui_screen(first: u16, images: oreui::images::OreUiImages) -> oreui::OreUiScreen {
    let mut screen = oreui::OreUiScreen::default();
    screen.look = oreui::Look::Originals;
    screen.originals = Some(Arc::new(oreui::Originals::new(first, images)));
    screen
}

#[cfg(test)]
mod tests;

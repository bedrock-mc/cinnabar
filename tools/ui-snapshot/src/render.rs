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
    for &reference in json_ui::ENGINE_SCREENS {
        registry.screen(reference);
    }
    ui_screens::death_screen::register(&mut registry);
    registry
        .catalog_mut()
        .layer_builtin(ui_screens::menu_policy::apply);
    registry
        .catalog_mut()
        .layer_builtin(ui_screens::menu_renderers::install_brand_overlays);
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
    let menu = matches!(&fixture, Fixture::Menu(_) | Fixture::Loading(_));
    let mut carriers = carriers::load(dir, menu)?;
    let size = state.size.map(|value| value as f32);
    let metrics = TextMetrics::for_viewport(
        state.size,
        DpiScale::new(1.0).unwrap(),
        Some(state.gui_scale),
    );
    let mut nodes = Vec::new();
    let mut next = 1;
    let mut session_pages = HashMap::new();
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
        Fixture::Menu(view) if view.json_ui_menu_reference().is_some() => {
            let data = ui_screens::menu_screens::screen_data(&view, frame.translate, 0)
                .ok_or_else(|| Error::Invalid("session menu has no JSON-UI screen data".into()))?;
            session_pages = session_menu(
                &data,
                &carriers.ui,
                carriers.ui_page,
                carriers.pages.len(),
                &mut frame,
            )?;
        }
        Fixture::Menu(view) => {
            carriers::validate_oreui(dir)?;
            let images = oreui::images::load_optional_oreui_images(dir)
                .ok_or_else(|| Error::Invalid("OreUI artwork is unusable; make assets".into()))?;
            let first = carriers.pages.len() as u16;
            let mut artwork: HashMap<_, _> = images
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
            mod_icons(&view, &mut artwork, &mut carriers.pages)?;
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
                let popup = if let Some(prompt) = view.server_trust_prompt() {
                    Some(launcher::menu::dialogs::server_trust(
                        &prompt.url,
                        frame.translate,
                    ))
                } else if let Some(name) = view.join_request_prompt() {
                    Some(launcher::menu::dialogs::join_request(name, frame.translate))
                } else {
                    view.dialog
                        .filter(|dialog| {
                            !matches!(
                                dialog,
                                launcher::menu::MenuDialog::MinecraftImport
                                    | launcher::menu::MenuDialog::Accounts
                                    | launcher::menu::MenuDialog::Exit
                            )
                        })
                        .map(|dialog| {
                            launcher::menu::dialogs::dialog(&view, dialog, frame.translate)
                        })
                };
                if let Some(popup) = popup {
                    screen
                        .paint_popup(&view, &popup, size, &mut frame)
                        .map_err(|error| error.to_string())?;
                }
                if view.dialog == Some(launcher::menu::MenuDialog::MinecraftImport) {
                    screen
                        .paint_minecraft_import(&view, size, &mut scrolls, &mut frame)
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        Fixture::Loading(stage) => {
            carriers::validate_oreui(dir)?;
            let images = oreui::images::load_optional_oreui_images(dir)
                .ok_or_else(|| Error::Invalid("OreUI artwork is unusable; make assets".into()))?;
            let first = carriers.pages.len() as u16;
            let artwork = HashMap::new();
            carriers.pages.extend(images.pages.iter().map(|page| Page {
                width: page.dimensions[0],
                height: page.dimensions[1],
                rgba: Arc::clone(&page.pixels),
            }));
            let mut screen = oreui_screen(first, images);
            let words = launcher::menu::join_progress::loading_words(stage, &|key, fallback| {
                ui_engine::engine::screen::translated(frame.translate, key, fallback)
            });
            frame.now = CAPTURE_SECONDS;
            screen
                .paint_loading(
                    oreui::loading_screen::LoadingInput {
                        stage,
                        words: [&words[0], &words[1]],
                        size,
                        title_artwork: None,
                        destination_icon: None,
                        artwork: &artwork,
                        dimension: 0,
                    },
                    &mut frame,
                )
                .map_err(|error| error.to_string())?;
        }
        Fixture::Chat {
            draft,
            suggestions,
            messages,
            options,
        } => {
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
            screen.settings.set_snapshot((options, None));
            for now in [0.0, CAPTURE_SECONDS] {
                frame.nodes.clear();
                *frame.next = 1;
                frame.now = now;
                screen
                    .paint(
                        &engine,
                        ChatInput {
                            editor: &editor,
                            messages: &messages,
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
    if let Some(batch) = list.batches.iter().find(|batch| {
        session_pages
            .get(&batch.texture_page)
            .or_else(|| carriers.pages.get(usize::from(batch.texture_page)))
            .is_none()
    }) {
        return Err(Error::Invalid(format!(
            "screen sampled unavailable texture page {} ({} loaded)",
            batch.texture_page,
            carriers.pages.len()
        )));
    }
    let mut canvas = Canvas::new(state.size[0], state.size[1]);
    canvas.draw(&list, |page| {
        session_pages
            .get(&page)
            .or_else(|| carriers.pages.get(usize::from(page)))
    });
    let mut pixels = canvas.into_rgba();
    if !menu {
        // An anonymous backdrop makes transparent in-world UI visible in an ordinary image viewer.
        for pixel in pixels.as_chunks_mut::<4>().0 {
            let alpha = f32::from(pixel[3]) / 255.0;
            for (channel, background) in [70.0, 90.0, 110.0].into_iter().enumerate() {
                pixel[channel] =
                    (f32::from(pixel[channel]) * alpha + background * (1.0 - alpha)).round() as u8;
            }
            pixel[3] = 255;
        }
    }
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            &pixels,
            state.size[0],
            state.size[1],
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| error.to_string())?;
    Ok(png)
}

/// Gives each Mods-tab icon its own page under the key its card draws, as the client's
/// artwork worker does.
fn mod_icons(
    view: &launcher::menu::MenuView,
    artwork: &mut HashMap<String, ui::IconRef>,
    pages: &mut Vec<Page>,
) -> Result<(), Error> {
    for item in &view.mods.library.installed {
        let (Some(key), Some(png)) = (item.icon_key(), &item.metadata.icon) else {
            continue;
        };
        let image = image::load_from_memory(png)
            .map_err(|error| error.to_string())?
            .to_rgba8();
        let (width, height) = image.dimensions();
        let page = u16::try_from(pages.len()).map_err(|error| error.to_string())?;
        artwork.insert(
            key,
            ui::IconRef {
                page,
                uv: [0, 0, width as u16, height as u16],
                glint: false,
            },
        );
        pages.push(Page {
            width,
            height,
            rgba: image.into_raw().into(),
        });
    }
    Ok(())
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

/// Paints an in-session menu and collects its logical texture pages without inventing missing images.
fn session_menu(
    data: &ui_screens::menu_screens::MenuScreenData,
    assets: &Arc<assets::RuntimeUiAssets>,
    ui_page: u16,
    first_dynamic: usize,
    frame: &mut PaintFrame<'_>,
) -> Result<HashMap<u16, Page>, Error> {
    let title = image::load_from_memory(launcher::branding::TITLE)
        .map_err(|error| error.to_string())?
        .to_rgba8();
    let title_page = u16::try_from(first_dynamic).map_err(|error| error.to_string())?;
    let artwork = HashMap::from([(
        ui_engine::textures::TITLE_KEY.into(),
        ui::IconRef {
            page: title_page,
            uv: [0, 0, title.width() as u16, title.height() as u16],
            glint: false,
        },
    )]);
    let first_dynamic = first_dynamic + 1;
    let catalog = json_ui::Catalog::from_files(
        assets
            .ui_files()
            .iter()
            .map(|file| (&*file.path, &*file.bytes)),
    )
    .map_err(|error| error.to_string())?;
    let mut engine = FormEngine::new(
        Arc::clone(assets),
        catalog,
        ui_page,
        &REGISTRY,
        Arc::new(Offline),
        ImageWork::inline(),
    );
    let first = u16::try_from(first_dynamic + ui_engine::pages::SERVER_UI_PAGE)
        .map_err(|error| error.to_string())?;
    engine.set_server_atlas(
        ui_engine::atlas::ServerAtlas::new(&[], None, ui_engine::pages::SERVER_UI_PAGES)
            .with_image_work(ImageWork::inline()),
        first,
    );
    frame.now = CAPTURE_SECONDS;
    engine
        .render_screen(
            data.reference,
            &data.data,
            &data.context,
            &json_ui::ViewState::default(),
            ui_engine::engine::ScreenArt {
                images: Some(&artwork),
                ..Default::default()
            },
            frame,
        )
        .map_err(|error| error.to_string())?;
    engine.take_server_pages();
    let mut pages = HashMap::from([(
        title_page,
        Page {
            width: title.width(),
            height: title.height(),
            rgba: title.into_raw().into(),
        },
    )]);
    for (offset, sources) in [
        (ui_engine::pages::SERVER_UI_PAGE, &engine.server_pages),
        (
            render_model::UI_NATIVE_SHEET_PAGE_OFFSET,
            &engine.server_sheet_pages,
        ),
    ] {
        for (index, page) in sources.iter().enumerate() {
            let [width, height] = page.dimensions();
            let logical =
                u16::try_from(first_dynamic + offset + index).map_err(|error| error.to_string())?;
            pages.insert(
                logical,
                Page {
                    width,
                    height,
                    rgba: page.pixels().into(),
                },
            );
        }
    }
    Ok(pages)
}

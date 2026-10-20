//! Production screen painting into the JSON-UI editor's existing CPU target.

mod character;
#[cfg(feature = "test-support")]
mod control_locked;
#[cfg(feature = "test-support")]
mod cosmetic_particles;
#[cfg(feature = "test-support")]
mod crafting;
mod edit_selection;
#[cfg(feature = "test-support")]
mod form_overlap;
mod panorama;

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
use ui_screens::{
    chat_screen::{ChatInput, ChatScreen},
    player_list::PlayerList,
};

use crate::{
    carriers::{self, Error},
    fixtures::{Fixture, State},
};

pub const CAPTURE_SECONDS: f64 = 1.0;

static REGISTRY: LazyLock<ScreenRegistry> = LazyLock::new(|| {
    let mut registry = ScreenRegistry::new(ui_screens::menu_screens::retail_context());
    registry.screen(ui_screens::chat_screen::CHAT_SCREEN);
    #[cfg(feature = "test-support")]
    registry.screen(
        ui_screens::containers::ScreenLayout::Personal { book: true }
            .screen()
            .0,
    );
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
#[cfg(test)]
pub fn png(state: &State, dir: &Path) -> Result<Vec<u8>, Error> {
    png_at(state, dir, CAPTURE_SECONDS)
}

/// Samples caller-owned UI and cosmetic animation time without reading the clock.
pub fn png_at(state: &State, dir: &Path, seconds: f64) -> Result<Vec<u8>, Error> {
    let fixture = (state.fixture)();
    #[cfg(feature = "test-support")]
    if matches!(&fixture, Fixture::ControlLocked) {
        return control_locked::png(state, dir);
    }
    #[cfg(feature = "test-support")]
    if let Fixture::CosmeticParticles { view, emitting } = &fixture {
        return cosmetic_particles::png(state.size, *view, seconds, *emitting);
    }
    #[cfg(feature = "test-support")]
    if matches!(&fixture, Fixture::FormOverlap) {
        return form_overlap::png(state, dir);
    }
    let menu = matches!(&fixture, Fixture::Menu(_) | Fixture::Loading(_));
    let show_panorama = match &fixture {
        Fixture::Menu(view) => view.out_of_game(),
        Fixture::Loading(stage) => stage.is_world_entry(),
        Fixture::Chat { .. } | Fixture::TabList(_) | Fixture::EditSelection => false,
        #[cfg(feature = "test-support")]
        Fixture::CraftOverlap { .. }
        | Fixture::CursorDurability
        | Fixture::FormOverlap
        | Fixture::ControlLocked
        | Fixture::CosmeticParticles { .. } => false,
    };
    let mut carriers = carriers::load(dir, menu)?;
    #[cfg(feature = "test-support")]
    let craft_icons = matches!(
        &fixture,
        Fixture::CraftOverlap { .. } | Fixture::CursorDurability
    )
    .then(|| crafting::icons(&mut carriers.pages))
    .transpose()?;
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
        #[cfg(feature = "test-support")]
        Fixture::CosmeticParticles { .. } | Fixture::FormOverlap | Fixture::ControlLocked => {
            unreachable!("specialized capture returned above")
        }
        #[cfg(feature = "test-support")]
        Fixture::CursorDurability => {
            let mut engine = json_engine(&carriers)?;
            crafting::paint_cursor(&engine, &carriers.lang, &craft_icons.unwrap(), &mut frame)?;
            add_server_pages(&mut engine, &mut carriers.pages);
        }
        #[cfg(feature = "test-support")]
        Fixture::CraftOverlap { selected } => {
            let mut engine = json_engine(&carriers)?;
            crafting::paint(
                &engine,
                &carriers.lang,
                &craft_icons.unwrap(),
                selected,
                &mut frame,
            )?;
            add_server_pages(&mut engine, &mut carriers.pages);
        }
        Fixture::EditSelection => {
            let mut engine = json_engine(&carriers)?;
            edit_selection::paint(&mut engine, &mut frame)?;
            add_server_pages(&mut engine, &mut carriers.pages);
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
            let logo = image::load_from_memory(launcher::branding::TITLE)
                .map_err(|error| Error::Invalid(format!("embedded title artwork: {error}")))?
                .into_rgba8();
            let title_artwork = ui::IconRef {
                page: carriers.pages.len() as u16,
                uv: [0, 0, logo.width() as u16, logo.height() as u16],
                glint: false,
            };
            carriers.pages.push(Page {
                width: logo.width(),
                height: logo.height(),
                rgba: logo.into_raw().into(),
            });
            cosmetic_icons(&mut artwork, &mut carriers.pages, seconds);
            mod_icons(&view, &mut artwork, &mut carriers.pages)?;
            let character_page = (view.screen == launcher::menu::MenuScreen::Home)
                .then(|| character::texture(dir, &mut carriers.pages))
                .transpose()?;
            let mut screen = oreui_screen(first, images);
            let mut scrolls = ui_engine::menu_scroll::MenuScrolls::default();
            // Mount at zero, then sample settled motion without reading a clock or sleeping.
            for now in [0.0, seconds] {
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
                            title_artwork: Some(title_artwork),
                            scrolls: &mut scrolls,
                            geometry: MenuGeometry::default(),
                        },
                        &mut frame,
                    )
                    .map_err(|error| error.to_string())?;
                let paint = screen.take_paint().unwrap();
                paint
                    .apply_entrance(frame.nodes, size)
                    .map_err(|error| error.to_string())?;
                if let (Some(preview), Some(page)) = (&paint.character_preview, character_page) {
                    character::paint(preview.control, page, &mut frame)?;
                }
                if view.dressing_room.editor.is_some() || view.dressing_room.cosmetic_filter_open {
                    screen
                        .paint_dressing_overlay(&paint, &view, size, &artwork, &mut frame)
                        .map_err(|error| error.to_string())?;
                }
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
                        .paint_popup(&view, &popup, size, &scrolls, &mut frame)
                        .map_err(|error| error.to_string())?;
                }
                if view.dialog == Some(launcher::menu::MenuDialog::MinecraftImport) {
                    screen
                        .paint_minecraft_import(&view, size, &mut scrolls, &mut frame)
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        Fixture::TabList(rows) => {
            carriers::install_default_skin(dir)?;
            let mut engine = json_engine(&carriers)?;
            let mut list = PlayerList::default();
            for now in [0.0, seconds] {
                frame.nodes.clear();
                *frame.next = 1;
                frame.now = now;
                list.paint(&engine, &rows, &mut frame)
                    .map_err(|error| error.to_string())?;
            }
            add_server_pages(&mut engine, &mut carriers.pages);
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
            frame.now = seconds;
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
            server_hover,
        } => {
            let mut engine = json_engine(&carriers)?;
            #[cfg(feature = "test-support")]
            if server_hover.is_some() {
                engine.set_server_pack(&[ui_screens::test_support::hover_chat::files()]);
            }

            let mut editor = ui::ChatEditor::new(ui::MAX_CHAT_INPUT_BYTES)
                .map_err(|error| format!("{error:?}"))?;
            editor.set_text(draft);
            frame.now = seconds;
            let mut screen = ChatScreen::default();
            screen.settings.set_snapshot((options, None));
            for now in [0.0, seconds, seconds] {
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
                if server_hover == Some(true)
                    && let Some(caption) = screen.nodes().iter().find(|node| node.name == "caption")
                {
                    screen.set_pointer(
                        ui::UiPoint::new(
                            (caption.dest.x as f32 + caption.dest.w as f32 - 4.0)
                                * metrics.gui_scale,
                            (caption.dest.y as f32 + 4.0) * metrics.gui_scale,
                        )
                        .ok(),
                    );
                }
            }
            if screen.nodes().is_empty() {
                return Err(Error::Invalid("native chat screen did not resolve".into()));
            }
            add_server_pages(&mut engine, &mut carriers.pages);
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
        carriers
            .pages
            .get(usize::from(batch.texture_page))
            .is_none()
    }) {
        return Err(Error::Invalid(format!(
            "screen sampled unavailable texture page {} ({} loaded)",
            batch.texture_page,
            carriers.pages.len()
        )));
    }
    let mut canvas = Canvas::new(state.size[0], state.size[1]);
    canvas.draw(&list, |page| carriers.pages.get(usize::from(page)));
    let mut pixels = canvas.into_rgba();
    if show_panorama {
        panorama::composite(&mut pixels, state.size, &carriers.ui)?;
    }
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

/// A JSON-UI engine over the vanilla carrier whose server atlas loads built-in images inline.
fn json_engine(carriers: &carriers::Carriers) -> Result<FormEngine, Error> {
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
    Ok(engine)
}

/// Appends the server atlas pages after painting; page order comes from the atlas.
fn add_server_pages(engine: &mut FormEngine, pages: &mut Vec<Page>) {
    if let Some(server) = engine.take_server_pages() {
        pages.extend(server.iter().map(|page| {
            let [width, height] = page.dimensions();
            Page {
                width,
                height,
                rgba: page.pixels().into(),
            }
        }));
    }
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

/// Uses the production cosmetic gallery renderer for original fixture artwork.
fn cosmetic_icons(artwork: &mut HashMap<String, ui::IconRef>, pages: &mut Vec<Page>, seconds: f64) {
    for item in cosmetics::Item::all() {
        let [width, height] = render_model::hats::thumbnail::SIZE;
        let Some(image) = render_model::hats::thumbnail::render_at(item, width, height, seconds)
        else {
            continue;
        };
        artwork.insert(
            item.thumbnail_key(),
            ui::IconRef {
                page: pages.len() as u16,
                uv: [0, 0, image.width as u16, image.height as u16],
                glint: false,
            },
        );
        pages.push(Page {
            width: image.width,
            height: image.height,
            rgba: image.rgba8,
        });
    }
}

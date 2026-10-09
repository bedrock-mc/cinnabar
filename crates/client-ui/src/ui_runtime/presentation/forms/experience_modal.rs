//! A client part's modal screen: its signed JSON-UI templates drawn through the engine over
//! gameplay. Templates resolve only against the vanilla catalog as the carrier ships it (no
//! server resource-pack layer) and the bundle's own files; textures come only from the bundle
//! and the vanilla pack. Cinnabar's trusted chrome is a separate catalog drawn afterwards, so
//! it stays on top and out of reach.

use std::{collections::BTreeMap, sync::Arc};

use json_ui::{
    ButtonInput, Catalog, CollectionItem, DataSource, Dispatcher, HitKind, InputMode, PointerInput,
    Scalar, ScreenEvent, ViewState,
};
use server_experience::{
    manifest::template_root,
    screen::{self, GuiSize},
};
use ui::UiNode;

use super::super::{FONT_DESIGN_PIXEL_TEXELS, IconRef, TextMetrics, UiPresentationRuntime};
use super::{
    engine::{EngineInputs, EngineOutput, ScreenArt},
    hud::CachedScreen,
    textures::TextureSet,
};
use crate::ui_runtime::{UiRuntime, forms::EngineFrame};

/// The modal a client part asks for: its bundle, verified screen files and bound data.
pub struct ExperienceModal<'a> {
    pub bundle: &'a str,
    pub files: &'a Arc<screen::Files>,
    pub modal: &'a screen::Modal,
}

pub(super) struct ModalScreen {
    bundle: String,
    files: Arc<screen::Files>,
    /// Vanilla plus the bundle's templates, built on first draw; `Err` names a rejected file.
    catalog: Option<Result<Arc<Catalog>, String>>,
    textures: Option<TextureSet>,
    /// The modal atlas's page images last handed to the dynamic pages.
    pages: Vec<render_model::UiTexturePage>,
    template: Option<String>,
    revision: Option<u64>,
    /// The client part's values and rows at `revision`.
    bound: DataSource,
    /// `bound` with the player's inventory rows, which the engine reads.
    data: Arc<DataSource>,
    /// The inventory and hotbar rows in `data`, and the icons their `#item_renderer_data`
    /// index; empty when `data` needs them added.
    rows: [Vec<CollectionItem>; 2],
    icons: Vec<IconRef>,
    screen: CachedScreen,
    view: ViewState,
    /// The pointer in virtual pixels; the view sees it only when a control follows it.
    pointer: Option<[f64; 2]>,
    frame: Option<EngineFrame>,
    /// The vanilla input components' state, which drives the edit boxes.
    dispatcher: Dispatcher,
    /// The last drawn root's size in GUI units and the GUI scale.
    size: Option<GuiSize>,
    /// The latest `Modal::texts` revision taken into `pending_texts`.
    texts_applied: u64,
    /// Host texts for edit boxes, by `text_box_name`, waiting for a drawn frame.
    pending_texts: Vec<(String, String)>,
    /// Each edit box's text last reported, by `text_box_name`.
    reported: BTreeMap<String, String>,
    /// The control a secondary press went down on.
    secondary: Option<String>,
    /// Each named scroll view's range last reported, by `scroll_view_name`.
    scrolled: BTreeMap<String, screen::ScrollRange>,
}

/// What one frame of input did to the modal's edit boxes.
#[derive(Debug, Default)]
pub struct ModalEdits {
    /// Each edited box's `text_box_name` and new text, once per box.
    pub edits: Vec<(String, String)>,
    /// Escape deselected a box, so it does not close the modal.
    pub escape_consumed: bool,
}

impl UiPresentationRuntime {
    /// Follows the client part's modal, closed or open; `None` (no running client part) drops
    /// its catalog and textures.
    pub fn set_experience_modal(&mut self, modal: Option<ExperienceModal<'_>>) {
        let slot = &mut self.form_presentation.experience_modal;
        let Some(modal) = modal else {
            *slot = None;
            return;
        };
        if slot.as_ref().is_none_or(|current| {
            current.bundle != modal.bundle || !Arc::ptr_eq(&current.files, modal.files)
        }) {
            *slot = Some(ModalScreen {
                bundle: modal.bundle.to_owned(),
                files: Arc::clone(modal.files),
                catalog: None,
                textures: None,
                pages: Vec::new(),
                template: None,
                revision: None,
                bound: DataSource::default(),
                data: Arc::default(),
                rows: Default::default(),
                icons: Vec::new(),
                screen: CachedScreen::default(),
                view: ViewState::default(),
                pointer: None,
                frame: None,
                dispatcher: Dispatcher::default(),
                size: None,
                texts_applied: 0,
                pending_texts: Vec::new(),
                reported: BTreeMap::new(),
                secondary: None,
                scrolled: BTreeMap::new(),
            });
        }
        let screen = slot.as_mut().expect("modal installed");
        if screen.template != modal.modal.template {
            screen.template.clone_from(&modal.modal.template);
            screen.view = ViewState::default();
            screen.frame = None;
            screen.dispatcher = Dispatcher::default();
            screen.reported.clear();
            screen.scrolled.clear();
        }
        if screen.revision != Some(modal.modal.revision) {
            screen.revision = Some(modal.modal.revision);
            screen.bound = data_source(modal.modal);
            screen.rows = Default::default();
            let mut texts: Vec<_> = modal
                .modal
                .texts
                .iter()
                .filter(|(_, (revision, _))| *revision > screen.texts_applied)
                .collect();
            texts.sort_by_key(|(_, (revision, _))| *revision);
            for (control, (revision, text)) in texts {
                screen.texts_applied = *revision;
                screen.pending_texts.push((control.clone(), text.clone()));
            }
        }
    }

    /// The drawn modal's root size in GUI units and the GUI scale; none while it is not drawn.
    pub fn experience_modal_size(&self) -> Option<GuiSize> {
        let screen = self.form_presentation.experience_modal.as_ref()?;
        screen.frame.as_ref().and(screen.size)
    }

    /// Drives the drawn modal's edit boxes as vanilla's `text_edit_box`: a primary press
    /// (`pressed`, at window-logical `point`) selects a box under it or, through the boxes' global
    /// mapping, deselects the selected one; `typed` goes to the selected box; Escape deselects
    /// it. Reports each box whose text changed, by its `text_box_name`.
    pub fn edit_experience_modal(
        &mut self,
        point: Option<[f32; 2]>,
        pressed: bool,
        typed: &[String],
        escape: bool,
        now: f64,
    ) -> ModalEdits {
        let mut out = ModalEdits::default();
        let Some(screen) = self.form_presentation.experience_modal.as_mut() else {
            return out;
        };
        let Some(frame) = &screen.frame else {
            return out;
        };
        let hits = &frame.hits;
        let point = point.map(|point| virtual_point(frame, point));
        let mut events = Vec::new();
        let on_box = point.is_some_and(|point| {
            hits.iter()
                .any(|region| region.kind == HitKind::EditBox && region.contains(point))
        });
        let selected = screen.view.components.selected().is_some();
        if pressed && (on_box || selected) {
            // The press answers a box's `pressed` mapping only over the hover chain, which the
            // dispatcher tracks; the modal's own hover and press stay as its buttons set them.
            let (hovered, held) = (screen.view.hovered.clone(), screen.view.pressed.clone());
            let pointer = PointerInput {
                point,
                held: false,
                mode: InputMode::Mouse,
                now,
            };
            screen.dispatcher.pointer(hits, &mut screen.view, pointer);
            (screen.view.hovered, screen.view.pressed) = (hovered, held);
            let press = ButtonInput {
                id: "button.menu_select",
                down: true,
                point,
                mode: InputMode::Mouse,
                now,
            };
            events.extend(
                screen
                    .dispatcher
                    .button(hits, &mut screen.view, press)
                    .events,
            );
        }
        for text in typed {
            events.extend(
                screen
                    .dispatcher
                    .text(hits, &mut screen.view, text, None)
                    .events,
            );
        }
        if escape && screen.view.components.selected().is_some() {
            for down in [true, false] {
                let cancel = ButtonInput {
                    id: "button.menu_cancel",
                    down,
                    point,
                    mode: InputMode::Mouse,
                    now,
                };
                let dispatch = screen.dispatcher.button(hits, &mut screen.view, cancel);
                out.escape_consumed |= dispatch.consumed;
                events.extend(dispatch.events);
            }
        }
        for event in events {
            let ScreenEvent::TextEdit { name, text, .. } = event else {
                continue;
            };
            if name.is_empty() || screen.reported.get(&name) == Some(&text) {
                continue;
            }
            screen.reported.insert(name.clone(), text.clone());
            out.edits.retain(|(control, _)| *control != name);
            out.edits.push((name, text));
        }
        out
    }

    /// Returns named scroll ranges whose last drawn values have not been accepted for delivery.
    /// Undelivered ranges are retried, with only the latest drawn range for each view.
    pub fn experience_modal_scrolls(&self) -> Vec<(String, screen::ScrollRange)> {
        let Some(screen) = self.form_presentation.experience_modal.as_ref() else {
            return Vec::new();
        };
        let Some(frame) = &screen.frame else {
            return Vec::new();
        };
        let mut out: Vec<(String, screen::ScrollRange)> = Vec::new();
        for region in frame.hits.iter() {
            let (HitKind::ScrollView, Some(name)) = (region.kind, &region.control_name) else {
                continue;
            };
            let Some(metrics) = frame.report.scrolls.get(&region.key) else {
                continue;
            };
            let range = screen::ScrollRange {
                offset: metrics.offset,
                viewport: metrics.viewport,
                content: metrics.content,
            };
            if !range.valid() {
                continue;
            }
            out.retain(|(view, _)| view != name);
            out.push((name.clone(), range));
        }
        out.retain(|(view, range)| screen.scrolled.get(view) != Some(range));
        out
    }

    /// Records a range accepted by the modal event queue; later changes remain pending.
    pub fn accept_experience_modal_scroll(&mut self, view: &str, range: screen::ScrollRange) {
        if let Some(screen) = self.form_presentation.experience_modal.as_mut() {
            screen.scrolled.insert(view.to_owned(), range);
        }
    }

    /// Why the modal's templates were refused, which ends the client part.
    pub fn experience_modal_failure(&self) -> Option<&str> {
        match &self.form_presentation.experience_modal.as_ref()?.catalog {
            Some(Err(error)) => Some(error),
            _ => None,
        }
    }

    /// Whether the client part has a screen open, drawn yet or not; Escape and
    /// `ui.close-screen` both close it.
    pub(super) fn experience_modal_open(&self) -> bool {
        self.form_presentation
            .experience_modal
            .as_ref()
            .is_some_and(|screen| screen.template.is_some())
    }

    /// Whether the last build drew the modal, which then owns pointer and keyboard.
    pub fn experience_modal_shown(&self) -> bool {
        self.form_presentation
            .experience_modal
            .as_ref()
            .is_some_and(|screen| screen.frame.is_some())
    }

    /// Lights the control under the pointer and remembers it for scrolling.
    pub fn hover_experience_modal(&mut self, point: Option<[f32; 2]>) {
        let Some(screen) = self.form_presentation.experience_modal.as_mut() else {
            return;
        };
        let Some(frame) = &screen.frame else {
            return;
        };
        let point = point.map(|point| virtual_point(frame, point));
        screen.pointer = point;
        screen.view.pointer = point.filter(|_| frame.report.tracks_pointer);
        screen.view.hovered = point.and_then(|point| {
            frame
                .hits
                .iter()
                .rev()
                .find(|region| region.enabled && region.pressed.is_some() && region.contains(point))
                .map(|region| region.key.clone())
        });
    }

    /// Scrolls the scroll view under the pointer by wheel `notches` (positive scrolls down).
    pub fn scroll_experience_modal(&mut self, notches: f64) {
        let Some(screen) = self.form_presentation.experience_modal.as_mut() else {
            return;
        };
        let (Some(frame), Some([x, y])) = (&screen.frame, screen.pointer) else {
            return;
        };
        if notches == 0.0 {
            return;
        }
        let under = frame.report.scrolls.iter().find(|(_, metrics)| {
            metrics
                .viewport_rect
                .is_some_and(|[left, top, width, height]| {
                    (left..=left + width).contains(&x) && (top..=top + height).contains(&y)
                })
        });
        if let Some((key, metrics)) = under {
            let offset =
                (metrics.offset + notches * metrics.speed).clamp(0.0, metrics.max_offset());
            screen.view.scroll.insert(key.clone(), offset);
        }
    }

    /// Tracks a left press and returns the control id and collection row of a press released
    /// over the control it began on, as vanilla buttons fire.
    pub fn press_experience_modal(
        &mut self,
        point: Option<[f32; 2]>,
        pressed: bool,
        released: bool,
    ) -> Option<(String, Option<usize>)> {
        let screen = self.form_presentation.experience_modal.as_mut()?;
        let frame = screen.frame.as_ref()?;
        let region =
            point.and_then(|point| {
                let point = virtual_point(frame, point);
                frame.hits.iter().rev().find(|region| {
                    region.enabled && region.pressed.is_some() && region.contains(point)
                })
            });
        if pressed {
            screen.view.pressed = region.map(|region| region.key.clone());
        }
        if !released {
            return None;
        }
        let held = screen.view.pressed.take()?;
        let region = region.filter(|region| region.key == held)?;
        Some((region.pressed.clone()?, region.collection_index))
    }

    /// Tracks a secondary (right) press and returns the action and collection row of one released
    /// over the control it began on, from that control's `button.menu_secondary_select` mapping,
    /// as vanilla controls map the secondary select.
    pub fn secondary_press_experience_modal(
        &mut self,
        point: Option<[f32; 2]>,
        pressed: bool,
        released: bool,
    ) -> Option<(String, Option<usize>)> {
        let screen = self.form_presentation.experience_modal.as_mut()?;
        let frame = screen.frame.as_ref()?;
        let target = |region: &json_ui::HitRegion| {
            region.input.mappings.iter().find_map(|mapping| {
                (mapping.from == "button.menu_secondary_select"
                    && mapping.kind == json_ui::MappingType::Pressed)
                    .then(|| mapping.to.clone())
            })
        };
        let region =
            point.and_then(|point| {
                let point = virtual_point(frame, point);
                frame.hits.iter().rev().find(|region| {
                    region.enabled && target(region).is_some() && region.contains(point)
                })
            });
        if pressed {
            screen.secondary = region.map(|region| region.key.clone());
        }
        if !released {
            return None;
        }
        let held = screen.secondary.take()?;
        let region = region.filter(|region| region.key == held)?;
        Some((target(region)?, region.collection_index))
    }

    /// Draws the modal over the gameplay scenes when nothing else holds the screen; trusted
    /// chrome draws after it.
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) fn append_experience_modal(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
        over_gameplay: bool,
    ) {
        let Some(screen) = self.form_presentation.experience_modal.as_mut() else {
            return;
        };
        screen.frame = None;
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
            return;
        };
        let Some(template) = screen.template.clone().filter(|_| over_gameplay) else {
            return;
        };
        // The player's inventory reads as vanilla's container screens bind it, replacing any
        // client part rows of those names; the data is rebuilt only when a row changed.
        let mut icons = Vec::new();
        let rows =
            super::containers::player_rows(player_runtime, runtime, &self.hud_frame, &mut icons);
        if screen.rows != rows || screen.icons != icons {
            let mut data = screen.bound.clone();
            let [inventory, hotbar] = rows.clone();
            data.set_collection("inventory_items", inventory);
            data.set_collection("hotbar_items", hotbar);
            screen.data = Arc::new(data);
            screen.rows = rows;
            screen.icons = icons;
        }
        let catalog = screen
            .catalog
            .get_or_insert_with(|| modal_catalog(&renderer.pack_catalog_base(), &screen.files));
        let Ok(catalog) = catalog.clone() else {
            return;
        };
        let page =
            (self.textures.dynamic_start() + super::super::dynamic_textures::MODAL_UI_PAGE) as u16;
        let textures = screen
            .textures
            .get_or_insert_with(|| renderer.textures.confined(&screen.files.textures, page));
        let reference = format!(
            "{}.{}",
            screen.files.namespace,
            template_root(&template).unwrap_or_default()
        );
        let translate = |key: &str| runtime.translation(key);
        let inputs = EngineInputs {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &translate,
            language: runtime.text_generation(),
        };
        let rollback = (nodes.len(), *next);
        let out = EngineOutput {
            nodes,
            next,
            overlay: &[],
        };
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let art = ScreenArt {
            icons: &screen.icons,
            view: Some(&screen.view),
            ..ScreenArt::default()
        };
        let data = Arc::clone(&screen.data);
        let result = renderer.draw_with(textures, art, inputs, out, |env, root| {
            screen.screen.render_shared_with(
                &reference,
                &catalog,
                renderer.context(),
                data,
                (root, px, runtime.text_generation()),
                env,
                &screen.view,
            )
        });
        match result {
            Ok(frame) => {
                screen.size = frame.as_ref().map(|frame| GuiSize {
                    width: f64::from(content[0] / px),
                    height: f64::from(content[1] / px),
                    scale: f64::from(frame.scale),
                });
                screen.frame = frame;
                if let Some(frame) = &screen.frame {
                    for (control, text) in screen.pending_texts.drain(..) {
                        screen.dispatcher.set_edit_text(
                            &frame.hits,
                            &mut screen.view,
                            &control,
                            &text,
                        );
                        screen.reported.insert(control, text);
                    }
                }
            }
            Err(error) => {
                nodes.truncate(rollback.0);
                *next = rollback.1;
                bevy::log::warn!(%error, bundle = %screen.bundle, "client part screen could not render");
            }
        }
    }

    /// Copies the modal atlas's page images when they changed; `true` asks for a page rebuild.
    pub(super) fn refresh_experience_modal_pages(&mut self) -> bool {
        let Some(screen) = self.form_presentation.experience_modal.as_mut() else {
            return false;
        };
        let Some(atlas) = screen.textures.as_mut().map(TextureSet::atlas_mut) else {
            return false;
        };
        if !atlas.take_dirty() {
            return false;
        }
        screen.pages = atlas.images().to_vec();
        true
    }

    /// The modal atlas's pages, for the dynamic pages reserved to it.
    pub(in super::super) fn experience_modal_pages(&self) -> &[render_model::UiTexturePage] {
        self.form_presentation
            .experience_modal
            .as_ref()
            .map_or(&[], |screen| &screen.pages)
    }
}

/// Every collection row and screen value as the engine's bindings read them.
fn data_source(modal: &screen::Modal) -> DataSource {
    let mut data = DataSource::new();
    for (name, value) in &modal.values {
        data.set_global(name.clone(), scalar(value));
    }
    for (name, rows) in &modal.collections {
        let items = rows
            .iter()
            .map(|row| {
                row.iter()
                    .fold(CollectionItem::default(), |item, (name, value)| {
                        item.with(name.clone(), scalar(value))
                    })
            })
            .collect();
        data.set_collection(name.clone(), items);
    }
    data
}

fn scalar(value: &screen::Value) -> Scalar {
    match value {
        screen::Value::Bool(value) => Scalar::Bool(*value),
        screen::Value::Integer(value) => Scalar::Int(*value),
        screen::Value::Number(value) => Scalar::Num(*value),
        screen::Value::Text(value) => Scalar::Text(value.clone()),
        screen::Value::Numbers(values) => Scalar::Json(values.as_slice().into()),
    }
}

fn virtual_point(frame: &EngineFrame, point: [f32; 2]) -> [f64; 2] {
    [
        f64::from((point[0] - frame.origin[0]) / frame.scale),
        f64::from((point[1] - frame.origin[1]) / frame.scale),
    ]
}

/// The vanilla catalog with the bundle's templates added in their own namespace. A namespace
/// the vanilla pack already has, a reference to a control vanilla lacks, or a template without
/// its root control is refused.
fn modal_catalog(vanilla: &Catalog, files: &screen::Files) -> Result<Arc<Catalog>, String> {
    let mut catalog = vanilla.clone();
    let before = catalog.namespace_count();
    for (path, bytes) in &files.templates {
        let references = screen::validate_template(bytes, &files.namespace)
            .map_err(|error| format!("{path}: {error}"))?;
        if let Some((namespace, name)) = references
            .iter()
            .find(|(namespace, name)| vanilla.lookup(namespace, name).is_none())
        {
            return Err(format!(
                "{path}: {namespace}.{name} is neither vanilla nor the bundle's"
            ));
        }
        catalog.overlay_text(path, &String::from_utf8_lossy(bytes));
    }
    if !files.templates.is_empty() && catalog.namespace_count() != before + 1 {
        return Err(format!(
            "namespace {} belongs to the vanilla pack",
            files.namespace
        ));
    }
    if let Some(path) = files.templates.keys().find(|path| {
        template_root(path).is_none_or(|root| catalog.lookup(&files.namespace, root).is_none())
    }) {
        return Err(format!("{path}: no root control named after the file"));
    }
    Ok(Arc::new(catalog))
}

#[cfg(test)]
mod tests;

//! A screen drawn from a package's own JSON-UI templates: a client part's modal, or a player
//! mod's overlay and view beside the container screens. Templates resolve only against the
//! vanilla catalog as the carrier ships it (no server resource-pack layer) and the package's own
//! files; textures come only from the package and the vanilla pack. Cinnabar's trusted chrome is
//! a separate catalog drawn afterwards, so it stays on top and out of reach.

use std::{collections::BTreeMap, sync::Arc};

use json_ui::{
    ButtonInput, Catalog, CollectionItem, DataSource, Dispatcher, HitKind, HitRegion, InputMode,
    PointerInput, Scalar, ScreenEvent, ViewState,
};
use server_experience::{
    manifest::template_root,
    screen::{self, GuiSize},
};

use super::{
    engine::{EngineInputs, EngineOutput, FormEngine, ScreenArt},
    hud::CachedScreen,
    textures::TextureSet,
};
use crate::ui_runtime::forms::EngineFrame;

/// A package's screen files with the catalog and textures drawn from them, built on first draw
/// and shared by every screen of the package.
pub(super) struct TemplateArt {
    pub(super) owner: String,
    pub(super) files: Arc<screen::Files>,
    /// Vanilla plus the package's templates; `Err` names a rejected file.
    catalog: Option<Result<Arc<Catalog>, String>>,
    textures: Option<TextureSet>,
    /// The atlas's page images last handed to the dynamic pages.
    pages: Vec<render_model::UiTexturePage>,
    /// The first dynamic page and the number of pages reserved to this package's atlas.
    page: u16,
    page_count: usize,
}

impl TemplateArt {
    pub(super) fn new(
        owner: &str,
        files: &Arc<screen::Files>,
        page: u16,
        page_count: usize,
    ) -> Self {
        Self {
            owner: owner.to_owned(),
            files: Arc::clone(files),
            catalog: None,
            textures: None,
            pages: Vec::new(),
            page,
            page_count,
        }
    }

    /// Whether this art is `owner`'s, built from `files`.
    pub(super) fn is(&self, owner: &str, files: &Arc<screen::Files>) -> bool {
        self.owner == owner && Arc::ptr_eq(&self.files, files)
    }

    /// Why the package's templates were refused.
    pub(super) fn failure(&self) -> Option<&str> {
        match &self.catalog {
            Some(Err(error)) => Some(error),
            _ => None,
        }
    }

    /// The catalog and textures, built on first use; `None` once the templates were refused.
    fn resolve(&mut self, renderer: &FormEngine) -> Option<(Arc<Catalog>, &TextureSet)> {
        let catalog = self
            .catalog
            .get_or_insert_with(|| package_catalog(&renderer.pack_catalog_base(), &self.files));
        let catalog = catalog.clone().ok()?;
        let textures = self.textures.get_or_insert_with(|| {
            renderer
                .textures
                .confined(&self.files.textures, self.page, self.page_count)
        });
        Some((catalog, textures))
    }

    /// Copies the atlas's page images when they changed; `true` asks for a page rebuild.
    pub(super) fn refresh_pages(&mut self) -> bool {
        let Some(atlas) = self.textures.as_mut().map(TextureSet::atlas_mut) else {
            return false;
        };
        if !atlas.take_dirty() {
            return false;
        }
        self.pages = atlas.images().to_vec();
        true
    }

    pub(super) fn pages(&self) -> &[render_model::UiTexturePage] {
        &self.pages
    }
}

/// One drawn template: its template, bound data, view state and the vanilla input components
/// that drive its edit boxes.
#[derive(Default)]
pub(super) struct TemplateScreen {
    pub(super) template: Option<String>,
    revision: Option<u64>,
    data: Arc<DataSource>,
    /// `data` with the components' writes (an edit box's text) last bound over it.
    bound: Option<(json_ui::Components, Arc<DataSource>)>,
    screen: CachedScreen,
    pub(super) view: ViewState,
    /// The pointer in virtual pixels; the view sees it only when a control follows it.
    pointer: Option<[f64; 2]>,
    pub(super) frame: Option<EngineFrame>,
    dispatcher: Dispatcher,
    /// The last drawn root's size in GUI units and the GUI scale.
    pub(super) size: Option<GuiSize>,
    /// The latest `Modal::texts` revision taken into `pending_texts`.
    texts_applied: u64,
    /// Host texts for edit boxes, by `text_box_name`, waiting for a drawn frame.
    pending_texts: Vec<(String, String)>,
    /// Each edit box's text last reported, by `text_box_name`.
    reported: BTreeMap<String, String>,
    /// The control a secondary press went down on.
    secondary: Option<String>,
    /// An edit box to select once drawn, by `text_box_name`.
    pending_focus: Option<String>,
}

/// What one frame of input did to a template screen's edit boxes.
#[derive(Debug, Default)]
pub struct ModalEdits {
    /// Each edited box's `text_box_name` and new text, once per box.
    pub edits: Vec<(String, String)>,
    /// Escape deselected a box, so it does not close the screen.
    pub escape_consumed: bool,
}

impl TemplateScreen {
    /// Follows the template and bound data, resetting the view for a new template.
    /// Returns whether a new data revision rebound values and queued host edit texts.
    pub(super) fn follow(&mut self, template: Option<&String>, modal: &screen::Modal) -> bool {
        if self.template.as_ref() != template {
            self.template = template.cloned();
            self.view = ViewState::default();
            self.frame = None;
            self.dispatcher = Dispatcher::default();
            self.reported.clear();
        }
        let changed = self.revision != Some(modal.revision);
        if changed {
            self.revision = Some(modal.revision);
            self.data = Arc::new(data_source(modal));
            self.bound = None;
            let mut texts: Vec<_> = modal
                .texts
                .iter()
                .filter(|(_, (revision, _))| *revision > self.texts_applied)
                .collect();
            texts.sort_by_key(|(_, (revision, _))| *revision);
            for (control, (revision, text)) in texts {
                self.texts_applied = *revision;
                self.pending_texts.push((control.clone(), text.clone()));
            }
        }
        changed
    }

    /// Replaces the modal's read-only player inventory rows and invalidates component bindings.
    pub(super) fn set_player_rows(&mut self, [inventory, hotbar]: [Vec<CollectionItem>; 2]) {
        let data = Arc::make_mut(&mut self.data);
        data.set_collection("inventory_items", inventory);
        data.set_collection("hotbar_items", hotbar);
        self.bound = None;
    }

    /// Selects the edit box named `control` once the screen is drawn.
    pub(super) fn focus(&mut self, control: &str) {
        self.pending_focus = Some(control.to_owned());
    }

    /// Whether an edit box of the drawn screen is selected and takes typing.
    pub(super) fn text_focused(&self) -> bool {
        self.frame.is_some() && self.view.components.selected().is_some()
    }

    /// Drives the drawn screen's edit boxes as vanilla's `text_edit_box`: a primary press
    /// (`pressed`, at window-logical `point`) selects a box under it or, through the boxes' global
    /// mapping, deselects the selected one; `typed` goes to the selected box; Escape deselects
    /// it. Reports each box whose text changed, by its `text_box_name`.
    pub(super) fn edit(
        &mut self,
        point: Option<[f32; 2]>,
        pressed: bool,
        typed: &[String],
        escape: bool,
        now: f64,
    ) -> ModalEdits {
        let mut out = ModalEdits::default();
        let Some(frame) = &self.frame else {
            return out;
        };
        let hits = &frame.hits;
        let point = point.map(|point| virtual_point(frame, point));
        let mut events = Vec::new();
        let on_box = point.is_some_and(|point| {
            hits.iter()
                .any(|region| region.kind == HitKind::EditBox && region.contains(point))
        });
        let selected = self.view.components.selected().is_some();
        if pressed && (on_box || selected) {
            // The press answers a box's `pressed` mapping only over the hover chain, which the
            // dispatcher tracks; the screen's own hover and press stay as its buttons set them.
            let (hovered, held) = (self.view.hovered.clone(), self.view.pressed.clone());
            let pointer = PointerInput {
                point,
                held: false,
                mode: InputMode::Mouse,
                now,
            };
            self.dispatcher.pointer(hits, &mut self.view, pointer);
            (self.view.hovered, self.view.pressed) = (hovered, held);
            let press = ButtonInput {
                id: "button.menu_select",
                down: true,
                point,
                mode: InputMode::Mouse,
                now,
            };
            events.extend(self.dispatcher.button(hits, &mut self.view, press).events);
        }
        for text in typed {
            events.extend(
                self.dispatcher
                    .text(hits, &mut self.view, text, None)
                    .events,
            );
        }
        if escape && self.view.components.selected().is_some() {
            for down in [true, false] {
                let cancel = ButtonInput {
                    id: "button.menu_cancel",
                    down,
                    point,
                    mode: InputMode::Mouse,
                    now,
                };
                let dispatch = self.dispatcher.button(hits, &mut self.view, cancel);
                out.escape_consumed |= dispatch.consumed;
                events.extend(dispatch.events);
            }
        }
        for event in events {
            let ScreenEvent::TextEdit { name, text, .. } = event else {
                continue;
            };
            if name.is_empty() || self.reported.get(&name) == Some(&text) {
                continue;
            }
            self.reported.insert(name.clone(), text.clone());
            out.edits.retain(|(control, _)| *control != name);
            out.edits.push((name, text));
        }
        out
    }

    /// Lights the control under the pointer and remembers it for scrolling.
    pub(super) fn hover(&mut self, point: Option<[f32; 2]>) {
        let Some(frame) = &self.frame else {
            return;
        };
        let point = point.map(|point| virtual_point(frame, point));
        self.pointer = point;
        self.view.pointer = point.filter(|_| frame.report.tracks_pointer);
        self.view.hovered = point.and_then(|point| {
            frame
                .hits
                .iter()
                .rev()
                .find(|region| region.enabled && region.pressed.is_some() && region.contains(point))
                .map(|region| region.key.clone())
        });
    }

    /// Scrolls the scroll view under the pointer by wheel `notches` (positive scrolls down);
    /// `true` when one took them.
    pub(super) fn scroll(&mut self, notches: f64) -> bool {
        let (Some(frame), Some([x, y])) = (&self.frame, self.pointer) else {
            return false;
        };
        if notches == 0.0 {
            return false;
        }
        let under = frame.report.scrolls.iter().find(|(_, metrics)| {
            metrics
                .viewport_rect
                .is_some_and(|[left, top, width, height]| {
                    (left..=left + width).contains(&x) && (top..=top + height).contains(&y)
                })
        });
        let Some((key, metrics)) = under else {
            return false;
        };
        let offset = (metrics.offset + notches * metrics.speed).clamp(0.0, metrics.max_offset());
        self.view.scroll.insert(key.clone(), offset);
        true
    }

    /// The drawn control under window-logical `point` that takes a primary press, edit boxes
    /// included.
    pub(super) fn control_at(&self, point: [f32; 2]) -> Option<&HitRegion> {
        let frame = self.frame.as_ref()?;
        let point = virtual_point(frame, point);
        frame.hits.iter().rev().find(|region| {
            region.enabled
                && (region.pressed.is_some()
                    || region.kind == HitKind::EditBox
                    || secondary_target(region).is_some())
                && region.contains(point)
        })
    }

    /// Tracks a left press and returns the control id and collection row of a press released
    /// over the control it began on, as vanilla buttons fire.
    pub(super) fn press(
        &mut self,
        point: Option<[f32; 2]>,
        pressed: bool,
        released: bool,
    ) -> Option<(String, Option<usize>)> {
        let frame = self.frame.as_ref()?;
        let region =
            point.and_then(|point| {
                let point = virtual_point(frame, point);
                frame.hits.iter().rev().find(|region| {
                    region.enabled && region.pressed.is_some() && region.contains(point)
                })
            });
        if pressed {
            self.view.pressed = region.map(|region| region.key.clone());
        }
        if !released {
            return None;
        }
        let held = self.view.pressed.take()?;
        let region = region.filter(|region| region.key == held)?;
        Some((region.pressed.clone()?, region.collection_index))
    }

    /// Tracks a secondary (right) press and returns the action and collection row of one
    /// released over the control it began on, from that control's
    /// `button.menu_secondary_select` mapping, as vanilla controls map the secondary select.
    pub(super) fn secondary_press(
        &mut self,
        point: Option<[f32; 2]>,
        pressed: bool,
        released: bool,
    ) -> Option<(String, Option<usize>)> {
        let frame = self.frame.as_ref()?;
        let region = point.and_then(|point| {
            let point = virtual_point(frame, point);
            frame.hits.iter().rev().find(|region| {
                region.enabled && secondary_target(region).is_some() && region.contains(point)
            })
        });
        if pressed {
            self.secondary = region.map(|region| region.key.clone());
        }
        if !released {
            return None;
        }
        let held = self.secondary.take()?;
        let region = region.filter(|region| region.key == held)?;
        Some((secondary_target(region)?, region.collection_index))
    }

    /// Draws the template over the whole content area; `false` when nothing was drawn (no
    /// template, a refused package, or the engine failing, whose nodes are rolled back).
    pub(super) fn draw(
        &mut self,
        art: &mut TemplateArt,
        renderer: &FormEngine,
        inputs: EngineInputs<'_>,
        (nodes, next): (&mut Vec<ui::UiNode>, &mut u32),
        screen_art: ScreenArt<'_>,
    ) -> bool {
        self.frame = None;
        let Some(template) = self.template.clone() else {
            return false;
        };
        let reference = format!(
            "{}.{}",
            art.files.namespace,
            template_root(&template).unwrap_or_default()
        );
        let owner = art.owner.clone();
        let Some((catalog, textures)) = art.resolve(renderer) else {
            return false;
        };
        let content = inputs.content;
        let px = inputs.metrics.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32;
        let language = inputs.language;
        let rollback = (nodes.len(), *next);
        let out = EngineOutput {
            nodes,
            next,
            overlay: &[],
        };
        let data = self.bound_data();
        let screen_art = ScreenArt {
            view: Some(&self.view),
            ..screen_art
        };
        let (screen, view) = (&mut self.screen, &self.view);
        let result = renderer.draw_with(textures, screen_art, inputs, out, |env, root| {
            screen.render_shared_with(
                &reference,
                &catalog,
                renderer.context(),
                data,
                (root, px, language),
                env,
                view,
            )
        });
        match result {
            Ok(frame) => {
                self.size = frame.as_ref().map(|frame| GuiSize {
                    width: f64::from(content[0] / px),
                    height: f64::from(content[1] / px),
                    scale: f64::from(frame.scale),
                });
                self.frame = frame;
                if let Some(frame) = &self.frame {
                    for (control, text) in self.pending_texts.drain(..) {
                        self.dispatcher
                            .set_edit_text(&frame.hits, &mut self.view, &control, &text);
                        self.reported.insert(control, text);
                    }
                }
                self.frame.is_some()
            }
            Err(error) => {
                nodes.truncate(rollback.0);
                *next = rollback.1;
                bevy::log::warn!(%error, %owner, "package screen could not render");
                false
            }
        }
    }
}

impl TemplateScreen {
    /// Releases selection removed from the allowed hits, then applies focus against those hits.
    pub(super) fn finish_input(&mut self, unfiltered: &[HitRegion]) {
        let lost = self.view.components.selected().is_some_and(|key| {
            self.frame
                .as_ref()
                .is_none_or(|frame| !frame.hits.iter().any(|hit| hit.enabled && hit.key == key))
        });
        if lost {
            self.dispatcher.clear_selection(unfiltered, &mut self.view);
        }
        if let Some(control) = self.pending_focus.take() {
            self.select_box(&control);
        }
    }

    /// The bound data with what the components wrote, such as an edit box's text, rebuilt only
    /// when either changed.
    fn bound_data(&mut self) -> Arc<DataSource> {
        if self.view.components.is_empty() {
            return Arc::clone(&self.data);
        }
        if let Some((components, data)) = &self.bound
            && *components == self.view.components
        {
            return Arc::clone(data);
        }
        let mut data = (*self.data).clone();
        data.set_components(self.view.components.clone());
        let data = Arc::new(data);
        self.bound = Some((self.view.components.clone(), Arc::clone(&data)));
        data
    }

    /// Selects the drawn edit box named `control` as a primary press on it would.
    fn select_box(&mut self, control: &str) {
        let Some(frame) = &self.frame else {
            return;
        };
        let Some(region) = frame.hits.iter().find(|region| {
            region.kind == HitKind::EditBox
                && region
                    .widget
                    .edit
                    .as_ref()
                    .is_some_and(|edit| edit.name.as_deref() == Some(control))
        }) else {
            return;
        };
        let [x, y, width, height] = [region.rect.x, region.rect.y, region.rect.w, region.rect.h];
        let point = [
            frame.origin[0] + ((x + width / 2.0) as f32) * frame.scale,
            frame.origin[1] + ((y + height / 2.0) as f32) * frame.scale,
        ];
        let _ = self.edit(Some(point), true, &[], false, 0.0);
    }
}

/// The declared action a control's `button.menu_secondary_select` mapping fires.
fn secondary_target(region: &HitRegion) -> Option<String> {
    region.input.mappings.iter().find_map(|mapping| {
        (mapping.from == "button.menu_secondary_select"
            && mapping.kind == json_ui::MappingType::Pressed)
            .then(|| mapping.to.clone())
    })
}

/// Every collection row and screen value as the engine's bindings read them.
pub(super) fn data_source(modal: &screen::Modal) -> DataSource {
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

pub(super) fn virtual_point(frame: &EngineFrame, point: [f32; 2]) -> [f64; 2] {
    [
        f64::from((point[0] - frame.origin[0]) / frame.scale),
        f64::from((point[1] - frame.origin[1]) / frame.scale),
    ]
}

/// The vanilla catalog with the package's templates added in their own namespace. A namespace
/// the vanilla pack already has, a reference to a control vanilla lacks, or a template without
/// its root control is refused.
pub(super) fn package_catalog(
    vanilla: &Catalog,
    files: &screen::Files,
) -> Result<Arc<Catalog>, String> {
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
                "{path}: {namespace}.{name} is neither vanilla nor the package's"
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

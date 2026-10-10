//! The server-form input model and the renderer that turns it into draw nodes
//! and hit regions. A [`FormModel`] describes a decoded server form (action,
//! modal, or custom); [`form_data_source`] maps it onto the `#binding` names the
//! vanilla templates read, and [`render_form_with`] resolves the template, binds,
//! lays out against the live [`ViewState`], and emits.
//!
//! Routing follows the vanilla client: action and custom forms open their
//! `server_form` factory templates, while a modal form opens the generic
//! two-button popup (`popup_dialog.modal_dialog_popup` with `$two_buttons_visible`),
//! its title/body/button texts fed as the popup's global values. Binding names are
//! read from the vanilla pack's `server_form.json`/`popup_dialog.json`.

use std::sync::Arc;

use crate::bind::{BindState, CollectionItem, ControlLibrary, DataSource, bind_stateful};
use crate::catalog::Catalog;
use crate::emit::{DrawNode, RectOut, emit};
use crate::input::{HitRegion, global_mapping, hit_regions};
use crate::layout::{LayoutEnv, MeasureCache, layout_with};
use crate::predicate::Scalar;
use crate::state::{LayoutReport, ViewState};
use crate::tree::{ControlRef, ResolvedControl};
use crate::{Context, resolve};

/// A decoded server form ready to render.
#[derive(Clone, Debug, PartialEq)]
pub enum FormModel {
    Action(ActionForm),
    Modal(ModalForm),
    Custom(CustomForm),
}

/// A button menu (`type:"form"`): title, body, and ordered elements. Only buttons
/// answer; labels, headers, and dividers decorate the list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActionForm {
    pub title: String,
    pub body: String,
    pub elements: Vec<ActionElement>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ActionElement {
    Button(FormButton),
    Label(String),
    Header(String),
    Divider,
}

/// A yes/no dialog (`type:"modal"`). `button1` answers `true`, `button2` `false`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModalForm {
    pub title: String,
    pub body: String,
    pub button1: String,
    pub button2: String,
}

/// An input form (`type:"custom_form"`): ordered elements plus the submit button.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomForm {
    pub title: String,
    pub icon: Option<ButtonImage>,
    pub elements: Vec<CustomElement>,
    pub submit_text: String,
    pub submit_visible: bool,
}

/// One action-form button: its label and an optional image.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormButton {
    pub text: String,
    pub image: Option<ButtonImage>,
}

/// A button image source, routed to the two texture bindings the template reads.
#[derive(Clone, Debug, PartialEq)]
pub enum ButtonImage {
    /// A resource-pack texture path → `#form_button_texture`.
    Path(String),
    /// A downloaded URL texture → both texture bindings.
    Url(String),
    /// A URL still downloading: `#form_button_texture` reads `loading`, which
    /// the vanilla template answers with its progress bar.
    Loading,
}

/// A custom-form element with its current (possibly user-edited) value. Display
/// text such as a slider's `label: value` is composed by the caller.
#[derive(Clone, Debug, PartialEq)]
pub enum CustomElement {
    Label {
        text: String,
    },
    Header {
        text: String,
    },
    Divider,
    Toggle {
        text: String,
        on: bool,
        tooltip: String,
    },
    Slider {
        text: String,
        /// Normalized `0..=1` position.
        fraction: f64,
        /// Seconds between repeated directional slider edits.
        timeout: f64,
        tooltip: String,
    },
    StepSlider {
        text: String,
        steps: usize,
        index: i32,
        tooltip: String,
    },
    Dropdown {
        text: String,
        options: Vec<String>,
        index: i32,
        open: bool,
        tooltip: String,
    },
    MultiSelect {
        text: String,
        options: Vec<String>,
        selected: Vec<i32>,
        open: bool,
        tooltip: String,
    },
    Input {
        text: String,
        value: String,
        placeholder: String,
        tooltip: String,
    },
}

/// The output of rendering a form.
#[derive(Debug)]
pub struct FormRender {
    /// The bound tree, for structural inspection.
    pub bound: ResolvedControl,
    pub nodes: Vec<DrawNode>,
    /// Immutable input regions shared by redraws of this layout.
    pub hits: Arc<[HitRegion]>,
    pub report: LayoutReport,
    /// Where `button.menu_cancel` (Escape/back) routes on this screen.
    pub cancel_target: Option<String>,
    /// The container screens' `root_panel` rect, the panel a click outside of
    /// drops the held stack from.
    pub root_panel: Option<RectOut>,
}

const LONG_FORM: &str = "server_form.long_form";
const CUSTOM_FORM: &str = "server_form.custom_form";
const MODAL_POPUP: &str = "popup_dialog.modal_dialog_popup";
const FORM_SCREEN: (&str, &str) = ("server_form", "third_party_server_screen");

/// The `server_form_factory` id the screen controller selects for a model; a
/// modal form opens the popup instead.
pub fn form_factory_id(model: &FormModel) -> Option<&'static str> {
    match model {
        FormModel::Action(_) => Some("long_form"),
        FormModel::Custom(_) => Some("custom_form"),
        FormModel::Modal(_) => None,
    }
}

/// The form screen's `$screen_content` (a resource pack may repoint it), the
/// control whose factory picks the long or custom form.
fn screen_content(catalog: &Catalog) -> Option<String> {
    let (screen, _) =
        crate::merge::flatten_def(catalog, FORM_SCREEN.0, FORM_SCREEN.1, &mut Vec::new())?;
    ["$screen_content", "$screen_content|default"]
        .iter()
        .find_map(|key| screen.props.get(*key)?.as_str().map(str::to_owned))
}

/// Where the form screen routes `button.menu_cancel` (Escape), from its own
/// global mappings, which sit above the content a form renders.
pub fn form_screen_cancel(catalog: &Catalog) -> Option<String> {
    let (screen, _) =
        crate::merge::flatten_def(catalog, FORM_SCREEN.0, FORM_SCREEN.1, &mut Vec::new())?;
    screen
        .props
        .get("button_mappings")?
        .as_array()?
        .iter()
        .find(|mapping| {
            mapping["from_button_id"] == "button.menu_cancel" && mapping["mapping_type"] == "global"
        })?
        .get("to_button_id")?
        .as_str()
        .map(str::to_owned)
}

/// The `namespace.name` of the vanilla template a model renders through.
pub fn form_template(model: &FormModel) -> &'static str {
    match model {
        FormModel::Action(_) => LONG_FORM,
        FormModel::Modal(_) => MODAL_POPUP,
        FormModel::Custom(_) => CUSTOM_FORM,
    }
}

/// The screen context a model resolves in: the caller's platform flags plus, for a
/// modal, the popup's two-button layout selection.
pub fn form_context(model: &FormModel, base: &Context) -> Context {
    let mut context = base.clone();
    if matches!(model, FormModel::Modal(_)) {
        for (flag, value) in [
            ("two_buttons_visible", true),
            ("no_buttons_visible", false),
            ("single_button_visible", false),
            ("single_button_checkbox_visible", false),
            ("two_buttons_checkbox_visible", false),
            ("destructive_two_buttons_visible", false),
            ("three_buttons_visible", false),
            ("destructive_three_buttons_visible", false),
            ("show_close_button", false),
        ] {
            context = context.with_flag(flag, value);
        }
        // The popup's button labels read their text from the controller, as a
        // screen hosting this popup selects.
        context = context.with_var(
            "button_text_binding_type",
            serde_json::Value::from("global"),
        );
    }
    context
}

/// The popup panel `popup_dialog.modal_dialog_popup`'s text views read.
const MODAL_SOURCE: &str = "modal_bg_buttons";

const PACK_TEXTURE_FS: &str = "InUserPackage";
const DOWNLOADED_TEXTURE_FS: &str = "RawPath";

/// Returns the texture reference and its pack or downloaded-file source.
fn image_bindings(image: Option<&ButtonImage>) -> (&str, &str) {
    match image {
        Some(ButtonImage::Path(path)) => (path, PACK_TEXTURE_FS),
        Some(ButtonImage::Url(url)) => (url, DOWNLOADED_TEXTURE_FS),
        Some(ButtonImage::Loading) => ("loading", PACK_TEXTURE_FS),
        None => ("", PACK_TEXTURE_FS),
    }
}

/// Map a form model onto the `#binding` names its template reads.
pub fn form_data_source(model: &FormModel) -> DataSource {
    let mut data = DataSource::new();
    // Server packs may inherit the shared container-only overlay in form content.
    data.set_global("#is_container_screen", Scalar::Bool(false));
    // Forms take pointer input, so gamepad-only chrome (focus outlines) stays hidden.
    data.set_global("#is_using_gamepad", Scalar::Bool(false));
    match model {
        FormModel::Action(form) => long_form_source(&mut data, form),
        FormModel::Modal(form) => {
            let text = |value: &str| Scalar::Text(value.to_owned());
            // The dialog's text views read the panel the controller fills.
            data.set_control_value(MODAL_SOURCE, "#modal_title_text", text(&form.title));
            data.set_control_value(MODAL_SOURCE, "#modal_label_text", text(&form.body));
            data.set_global("#modal_left_button_text", text(&form.button1));
            data.set_global("#modal_middle_button_text", text(""));
            data.set_global("#modal_rightcancel_button_text", text(&form.button2));
        }
        FormModel::Custom(form) => custom_form_source(&mut data, form),
    }
    data
}

fn long_form_source(data: &mut DataSource, form: &ActionForm) {
    data.set_creation_value("#title_text", Scalar::Text(form.title.clone()));
    data.set_creation_value("#form_text", Scalar::Text(form.body.clone()));
    data.set_global("#title_text", Scalar::Text(form.title.clone()));
    data.set_global("#form_text", Scalar::Text(form.body.clone()));
    let length = Scalar::Num(form.elements.len() as f64);
    data.set_global("#form_button_length", length);
    data.set_global("#submit_button_visible", Scalar::Bool(true));
    let text_item = |role: &str, text: &str| {
        CollectionItem::new(role).with("#form_button_text", Scalar::Text(text.to_owned()))
    };
    let items: Vec<_> = form
        .elements
        .iter()
        .map(|element| match element {
            ActionElement::Button(button) => {
                let (path, file_system) = image_bindings(button.image.as_ref());
                text_item("button", &button.text)
                    .with("#form_button_texture", Scalar::Text(path.to_owned()))
                    .with(
                        "#form_button_texture_file_system",
                        Scalar::Text(file_system.to_owned()),
                    )
            }
            ActionElement::Label(text) => text_item("label", text),
            ActionElement::Header(text) => text_item("header", text),
            ActionElement::Divider => CollectionItem::new("divider"),
        })
        .collect();
    let contents = items
        .iter()
        .map(|item| {
            item.role
                .clone()
                .map_or(serde_json::Value::Null, serde_json::Value::String)
        })
        .collect();
    data.set_global(
        "#form_button_contents",
        Scalar::Json(serde_json::Value::Array(contents)),
    );
    data.set_collection("form_buttons", items);
}

fn custom_form_source(data: &mut DataSource, form: &CustomForm) {
    let (icon, file_system) = image_bindings(form.icon.as_ref());
    data.set_global("#server_icon", Scalar::Text(icon.to_owned()));
    data.set_global("#server_outline_icon", Scalar::Text(icon.to_owned()));
    data.set_global(
        "#server_icon_file_system",
        Scalar::Text(file_system.to_owned()),
    );
    data.set_creation_value("#title_text", Scalar::Text(form.title.clone()));
    data.set_global("#title_text", Scalar::Text(form.title.clone()));
    data.set_global(
        "#custom_form_length",
        Scalar::Num(form.elements.len() as f64),
    );
    data.set_global("#submit_text", Scalar::Text(form.submit_text.clone()));
    data.set_global("#submit_button_visible", Scalar::Bool(form.submit_visible));
    let items = form.elements.iter().map(custom_item).collect();
    data.set_collection("custom_form", items);
    for (parent, element) in form.elements.iter().enumerate() {
        match element {
            CustomElement::MultiSelect {
                options, selected, ..
            } => {
                let selected: std::collections::BTreeSet<_> = selected
                    .iter()
                    .copied()
                    .filter(|index| *index >= 0 && (*index as usize) < options.len())
                    .collect();
                data.set_scoped_collection(
                    "custom_form",
                    parent,
                    "custom_multiselect",
                    option_items(
                        options,
                        "checkbox",
                        "#custom_multiselect_text",
                        "#custom_multiselect_toggled",
                        |index| selected.contains(&(index as i32)),
                    ),
                );
            }
            CustomElement::Dropdown { options, index, .. } => data.set_scoped_collection(
                "custom_form",
                parent,
                "custom_dropdown",
                option_items(
                    options,
                    "radio",
                    "#custom_radio_text",
                    "#custom_radio_toggled",
                    |position| position as i32 == *index,
                ),
            ),
            _ => {}
        }
    }
}

/// Builds one parent's option rows with their display text and checked state.
fn option_items(
    options: &[String],
    role: &str,
    text: &str,
    checked: &str,
    selected: impl Fn(usize) -> bool,
) -> Vec<CollectionItem> {
    options
        .iter()
        .enumerate()
        .map(|(index, option)| {
            CollectionItem::new(role)
                .with(text, Scalar::Text(option.clone()))
                .with(checked, Scalar::Bool(selected(index)))
        })
        .collect()
}

fn custom_item(element: &CustomElement) -> CollectionItem {
    let text = |value: &str| Scalar::Text(value.to_owned());
    match element {
        CustomElement::Label { text: label } => {
            CollectionItem::new("label").with("#custom_text", text(label))
        }
        CustomElement::Header { text: label } => {
            CollectionItem::new("header").with("#custom_text", text(label))
        }
        CustomElement::Divider => CollectionItem::new("divider"),
        CustomElement::Toggle {
            text: label,
            on,
            tooltip,
        } => CollectionItem::new("toggle")
            .with("#custom_text", text(label))
            .with("#custom_toggle_state", Scalar::Bool(*on))
            .with("#custom_toggle_enabled", Scalar::Bool(true))
            .with("#custom_tooltip_text", text(tooltip)),
        CustomElement::Slider {
            text: label,
            fraction,
            timeout,
            tooltip,
        } => CollectionItem::new("slider")
            .with("#custom_slider_text", text(label))
            .with("#custom_slider_text_value", text(label))
            .with(
                "#custom_slider_value",
                Scalar::Num(fraction.clamp(0.0, 1.0)),
            )
            .with("#custom_slider_timeout", Scalar::Num(*timeout))
            .with("#custom_slider_enabled", Scalar::Bool(true))
            .with("#custom_tooltip_text", text(tooltip)),
        CustomElement::StepSlider {
            text: label,
            steps,
            index,
            tooltip,
        } => CollectionItem::new("step_slider")
            .with("#custom_slider_step_text", text(label))
            .with("#custom_slider_step_text_value", text(label))
            .with("#custom_slider_step_value", Scalar::Num(*index as f64))
            .with("#custom_slider_steps", Scalar::Num((*steps).max(1) as f64))
            .with("#custom_slider_enabled", Scalar::Bool(true))
            .with("#custom_tooltip_text", text(tooltip)),
        CustomElement::Dropdown {
            text: label,
            options,
            index,
            open,
            tooltip,
        } => CollectionItem::new("dropdown")
            .with("#custom_text", text(label))
            .with(
                "#dropdown_option_text",
                text(options.get(*index as usize).map_or("", String::as_str)),
            )
            .with("#custom_dropdown", Scalar::Bool(*open))
            .with("#custom_dropdown_length", Scalar::Num(options.len() as f64))
            .with("#custom_toggle_enabled", Scalar::Bool(true))
            .with("#custom_tooltip_text", text(tooltip)),
        CustomElement::MultiSelect {
            text: label,
            options,
            open,
            tooltip,
            ..
        } => CollectionItem::new("multiselect")
            .with("#custom_text", text(label))
            .with("#custom_multiselect", Scalar::Bool(*open))
            .with(
                "#custom_multiselect_length",
                Scalar::Num(options.len() as f64),
            )
            .with("#custom_toggle_enabled", Scalar::Bool(true))
            .with("#custom_tooltip_text", text(tooltip)),
        CustomElement::Input {
            text: label,
            value,
            placeholder,
            tooltip,
        } => CollectionItem::new("input")
            .with("#custom_text", text(label))
            .with("#custom_input_text", text(value))
            .with("#custom_placeholder_text", text(placeholder))
            .with("#custom_input_enabled", Scalar::Bool(true))
            .with("#custom_tooltip_text", text(tooltip)),
    }
}

/// A [`ControlLibrary`] over the pack catalog: factory `control_ids` resolve as
/// standalone `namespace.name` templates in `context`.
pub struct CatalogLibrary<'a> {
    pub catalog: &'a Catalog,
    pub context: &'a Context,
}

impl ControlLibrary for CatalogLibrary<'_> {
    fn resolve(&self, reference: &ControlRef) -> Option<ResolvedControl> {
        let name = format!("{}.{}", reference.namespace, reference.name);
        resolve(self.catalog, &name, self.context).control
    }

    fn resolve_with(
        &self,
        reference: &ControlRef,
        _key: &str,
        vars: &dyn Fn() -> std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Option<ResolvedControl> {
        let mut context = self.context.clone();
        for (name, value) in vars() {
            context = context.with_var(name.trim_start_matches('$'), value);
        }
        let name = format!("{}.{}", reference.namespace, reference.name);
        resolve(self.catalog, &name, &context).control
    }
}

/// Factory and grid resolutions kept across binds of one catalog and context,
/// so a screen re-bound for changed data reuses its created controls' trees.
#[derive(Default)]
pub struct ResolveCache {
    memo: std::sync::Mutex<Option<ResolveMemo>>,
}

/// Recent resolutions and the catalog-and-context root scope they resolve in.
struct ResolveMemo {
    trees: crate::lru::Lru<(ControlRef, String), Option<Arc<ResolvedControl>>>,
    root: crate::env::Env,
}

/// Distinct factory resolutions kept; text-carrying `$vars` (titles, the action
/// bar) make one per message.
const RESOLUTIONS: usize = 512;

impl std::fmt::Debug for ResolveCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolveCache").finish_non_exhaustive()
    }
}

/// A [`CatalogLibrary`] answering from a [`ResolveCache`] first.
pub struct CachedLibrary<'a> {
    pub library: CatalogLibrary<'a>,
    pub cache: &'a ResolveCache,
}

impl ControlLibrary for CachedLibrary<'_> {
    fn resolve(&self, reference: &ControlRef) -> Option<ResolvedControl> {
        self.resolve_with(reference, "", &std::collections::BTreeMap::new)
    }

    fn resolve_with(
        &self,
        reference: &ControlRef,
        key: &str,
        vars: &dyn Fn() -> std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Option<ResolvedControl> {
        self.resolve_shared(reference, key, vars)
            .map(Arc::unwrap_or_clone)
    }

    fn resolve_shared(
        &self,
        reference: &ControlRef,
        key: &str,
        vars: &dyn Fn() -> std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Option<Arc<ResolvedControl>> {
        let cache_key = (reference.clone(), key.to_owned());
        let Ok(mut memo) = self.cache.memo.lock() else {
            return self
                .library
                .resolve_with(reference, key, vars)
                .map(Arc::new);
        };
        let memo = memo.get_or_insert_with(|| ResolveMemo {
            trees: crate::lru::Lru::new(RESOLUTIONS),
            root: self.library.context.root_env(self.library.catalog),
        });
        if let Some(resolved) = memo.trees.get(&cache_key) {
            return resolved.clone();
        }
        let vars = vars();
        let name = format!("{}.{}", reference.namespace, reference.name);
        // A null `$var` reads as unset rather than masking a global, which a
        // scope over the shared root cannot express.
        let resolved = if vars.values().any(serde_json::Value::is_null) {
            self.library.resolve_with(reference, key, &|| vars.clone())
        } else {
            let mut root = memo.root.child();
            for (name, value) in vars {
                root.set(name.trim_start_matches('$'), value);
            }
            crate::resolve_in(self.library.catalog, &name, &root.settle()).control
        }
        .map(Arc::new);
        memo.trees.insert(cache_key, resolved.clone());
        resolved
    }
}

/// Resolve the model's template and bind it against the mapped data source,
/// returning the baked tree. `None` when the template is unknown or a binding
/// budget would omit part of the form.
pub fn bind_form(
    model: &FormModel,
    catalog: &Catalog,
    context: &Context,
) -> Option<ResolvedControl> {
    bind_form_over(model, catalog, context, &crate::Components::default())
}

/// [`bind_form`] over what the form's components wrote into their bags.
pub fn bind_form_over(
    model: &FormModel,
    catalog: &Catalog,
    context: &Context,
    components: &crate::Components,
) -> Option<ResolvedControl> {
    let capacity = crate::bind::feed::MAX_FACTORY_ITEMS;
    let within_capacity = match model {
        FormModel::Action(form) => form.elements.len() <= capacity,
        FormModel::Custom(form) => {
            form.elements.len() <= capacity
                && form.elements.iter().all(|element| match element {
                    CustomElement::Dropdown { options, .. }
                    | CustomElement::MultiSelect { options, .. } => options.len() <= capacity,
                    _ => true,
                })
        }
        FormModel::Modal(_) => true,
    };
    if !within_capacity {
        return None;
    }
    let context = form_context(model, context);
    let mut data = form_data_source(model);
    data.set_components(components.clone());
    // Action and custom forms open through the screen's content factory, so a
    // pack's screen override applies; the bare template is the fallback.
    let routed = form_factory_id(model).and_then(|id| {
        let content = screen_content(catalog)?;
        let root = resolve(catalog, &content, &context).control?;
        data.set_factory_id(id);
        Some(root)
    });
    let root = match routed {
        Some(root) => root,
        None => resolve(catalog, form_template(model), &context).control?,
    };
    let library = CatalogLibrary {
        catalog,
        context: &context,
    };
    let mut state = BindState::new();
    let (bound, _) = bind_stateful(&Arc::new(root), &data, &library, &mut state);
    (!state.node_budget_exceeded).then_some(bound)
}

/// Render a form with no interaction state.
pub fn render_form(
    model: &FormModel,
    catalog: &Catalog,
    context: &Context,
    root_size: [f64; 2],
    env: &LayoutEnv,
) -> Option<FormRender> {
    render_form_with(
        model,
        catalog,
        context,
        root_size,
        env,
        &ViewState::default(),
    )
}

/// Render a form within a `root_size` virtual screen under live `state`.
pub fn render_form_with(
    model: &FormModel,
    catalog: &Catalog,
    context: &Context,
    root_size: [f64; 2],
    env: &LayoutEnv,
    state: &ViewState,
) -> Option<FormRender> {
    let bound = bind_form(model, catalog, context)?;
    Some(finish(bound, root_size, env, state))
}

/// Lay out, emit, and collect input for a tree [`bind_form`] already bound, so a
/// caller can re-lay out under new view state without re-resolving.
pub fn render_bound(
    bound: ResolvedControl,
    root_size: [f64; 2],
    env: &LayoutEnv,
    state: &ViewState,
) -> FormRender {
    finish(bound, root_size, env, state)
}

/// Render a stable tree with measurements retained by the caller across data updates.
/// Call `MeasureCache::update_tree` before changing the tree; reset the cache when
/// the root size or measurement environment changes.
pub fn render_bound_cached(
    bound: ResolvedControl,
    root_size: [f64; 2],
    env: &LayoutEnv,
    state: &ViewState,
    measures: &mut MeasureCache,
) -> FormRender {
    lay_out_and_emit(bound, root_size, env, state, Some((measures, false)))
}

/// [`render_bound`] independent of hover, press and focus: scrolling, pointer
/// tracking and dragging still affect layout. State children emit gated
/// ([`crate::emit_gated`]), so the result stays valid while the data, layout
/// state, root size and measurement environment stay unchanged. Filter
/// its nodes with [`DrawNode::shown`]; its hit regions are the neutral state's,
/// less scroll content wholly outside its viewport, which is not laid out.
/// `measures` must belong to this tree, root size and `env`.
pub fn render_bound_gated(
    bound: ResolvedControl,
    root_size: [f64; 2],
    env: &LayoutEnv,
    state: &ViewState,
    measures: &mut MeasureCache,
) -> FormRender {
    let neutral = state.layout_part();
    lay_out_and_emit(bound, root_size, env, &neutral, Some((measures, true)))
}

/// Lay out, emit, and collect input for a bound tree.
pub(crate) fn finish(
    bound: ResolvedControl,
    root_size: [f64; 2],
    env: &LayoutEnv,
    state: &ViewState,
) -> FormRender {
    lay_out_and_emit(bound, root_size, env, state, None)
}

fn lay_out_and_emit(
    bound: ResolvedControl,
    root_size: [f64; 2],
    env: &LayoutEnv,
    state: &ViewState,
    gated: Option<(&mut MeasureCache, bool)>,
) -> FormRender {
    let (nodes, hits, report, cancel_target, root_panel) = {
        let (laid, report, gate) = match gated {
            Some((measures, false)) => {
                let (output, report) =
                    crate::layout::layout_reusing(&bound, root_size, env, state, measures);
                return FormRender {
                    bound,
                    nodes: output.nodes,
                    hits: output.hits.into(),
                    report,
                    cancel_target: output.cancel,
                    root_panel: output.root_panel,
                };
            }
            Some((measures, true)) => {
                let (laid, report) =
                    crate::layout::layout_culled(&bound, root_size, env, state, measures);
                (laid, report, true)
            }
            None => {
                let (laid, report) = layout_with(&bound, root_size, env, state);
                (laid, report, false)
            }
        };
        (
            if gate {
                crate::emit::emit_gated(&laid, env)
            } else {
                emit(&laid, env)
            },
            hit_regions(&laid).into(),
            report,
            global_mapping(&laid, "button.menu_cancel"),
            find_rect(&laid, "root_panel"),
        )
    };
    FormRender {
        bound,
        nodes,
        hits,
        report,
        cancel_target,
        root_panel,
    }
}

fn find_rect(node: &crate::layout::LaidOut, name: &str) -> Option<RectOut> {
    if node.control.name == name && node.visible {
        return Some(node.rect.into());
    }
    node.children
        .iter()
        .find_map(|child| find_rect(child, name))
}

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

use crate::vanilla::{Binding, Button, Control, Reference, Variable};
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

#[cfg(test)]
#[path = "form/library_tests.rs"]
mod library_tests;

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
        enabled: bool,
        on: bool,
        tooltip: String,
    },
    Slider {
        text: String,
        enabled: bool,
        /// Normalized `0..=1` position.
        fraction: f64,
        /// Seconds between repeated directional slider edits.
        timeout: f64,
        tooltip: String,
    },
    StepSlider {
        text: String,
        enabled: bool,
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
        enabled: bool,
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
    /// Immutable emitted nodes; sharing this snapshot preserves its layout identity.
    pub nodes: Arc<[DrawNode]>,
    /// Immutable input regions shared by redraws of this layout.
    pub hits: Arc<[HitRegion]>,
    pub report: LayoutReport,
    /// Where `button.menu_cancel` (Escape/back) routes on this screen.
    pub cancel_target: Option<String>,
    /// The container screens' first visible `root_panel` rect.
    pub root_panel: Option<RectOut>,
}

/// The `server_form_factory` id the screen controller selects for a model; a
/// modal form opens the popup instead.
pub fn form_factory_id(model: &FormModel) -> Option<&'static str> {
    match model {
        FormModel::Action(_) => Some("long_form"),
        FormModel::Custom(_) => Some("custom_form"),
        FormModel::Modal(_) => None,
    }
}

/// Resolves the content factory under the form screen's selected variables.
fn screen_content(catalog: &Catalog, context: &Context) -> Option<ResolvedControl> {
    let (namespace, name) = Reference::ServerFormThirdPartyServerScreen
        .as_str()
        .split_once('.')?;
    crate::Resolver::new(catalog).resolve_variable_content(
        namespace,
        name,
        Variable::ScreenContent.as_str(),
        &context.root_env(catalog),
    )
}

/// Where the form screen routes `button.menu_cancel` (Escape), from its own
/// global mappings, which sit above the content a form renders.
pub fn form_screen_cancel(catalog: &Catalog) -> Option<String> {
    let (namespace, name) = Reference::ServerFormThirdPartyServerScreen
        .as_str()
        .split_once('.')?;
    let (screen, _) = crate::merge::flatten_def(catalog, namespace, name, &mut Vec::new())?;
    screen
        .props
        .get("button_mappings")?
        .as_array()?
        .iter()
        .find(|mapping| {
            mapping["from_button_id"] == Button::MenuCancel.as_str()
                && mapping["mapping_type"] == "global"
        })?
        .get("to_button_id")?
        .as_str()
        .map(str::to_owned)
}

/// The `namespace.name` of the vanilla template a model renders through.
pub fn form_template(model: &FormModel) -> &'static str {
    match model {
        FormModel::Action(_) => Reference::ServerFormLongForm.as_str(),
        FormModel::Modal(_) => Reference::PopupDialogModalDialogPopup.as_str(),
        FormModel::Custom(_) => Reference::ServerFormCustomForm.as_str(),
    }
}

/// The screen context a model resolves in: the caller's platform flags plus, for a
/// modal, the popup's two-button layout selection.
pub fn form_context(model: &FormModel, base: &Context) -> Context {
    let mut context = base.clone();
    if matches!(model, FormModel::Modal(_)) {
        for (flag, value) in [
            (Variable::TwoButtonsVisible.key(), true),
            (Variable::NoButtonsVisible.key(), false),
            (Variable::SingleButtonVisible.key(), false),
            (Variable::SingleButtonCheckboxVisible.key(), false),
            (Variable::TwoButtonsCheckboxVisible.key(), false),
            (Variable::DestructiveTwoButtonsVisible.key(), false),
            (Variable::ThreeButtonsVisible.key(), false),
            (Variable::DestructiveThreeButtonsVisible.key(), false),
            (Variable::ShowCloseButton.key(), false),
        ] {
            context = context.with_flag(flag, value);
        }
        // The popup's button labels read their text from the controller, as a
        // screen hosting this popup selects.
        context = context.with_var(
            Variable::ButtonTextBindingType.key(),
            serde_json::Value::from("global"),
        );
    }
    context
}

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
    data.set_global(Binding::IsContainerScreen.as_str(), Scalar::Bool(false));
    // Forms take pointer input, so gamepad-only chrome (focus outlines) stays hidden.
    data.set_global(Binding::IsUsingGamepad.as_str(), Scalar::Bool(false));
    match model {
        FormModel::Action(form) => long_form_source(&mut data, form),
        FormModel::Modal(form) => {
            let text = |value: &str| Scalar::Text(value.to_owned());
            // The dialog's text views read the panel the controller fills.
            data.set_control_value(
                Control::ModalBgButtons.as_str(),
                Binding::ModalTitleText.as_str(),
                text(&form.title),
            );
            data.set_control_value(
                Control::ModalBgButtons.as_str(),
                Binding::ModalLabelText.as_str(),
                text(&form.body),
            );
            data.set_global(Binding::ModalLeftButtonText.as_str(), text(&form.button1));
            data.set_global(Binding::ModalMiddleButtonText.as_str(), text(""));
            data.set_global(
                Binding::ModalRightcancelButtonText.as_str(),
                text(&form.button2),
            );
        }
        FormModel::Custom(form) => custom_form_source(&mut data, form),
    }
    data
}

fn long_form_source(data: &mut DataSource, form: &ActionForm) {
    data.set_creation_value(
        Binding::TitleText.as_str(),
        Scalar::Text(form.title.clone()),
    );
    data.set_creation_value(Binding::FormText.as_str(), Scalar::Text(form.body.clone()));
    data.set_global(
        Binding::TitleText.as_str(),
        Scalar::Text(form.title.clone()),
    );
    data.set_global(Binding::FormText.as_str(), Scalar::Text(form.body.clone()));
    let length = Scalar::Num(form.elements.len() as f64);
    data.set_global("#form_button_length", length);
    data.set_global(Binding::SubmitButtonVisible.as_str(), Scalar::Bool(true));
    let text_item = |role: &str, text: &str| {
        CollectionItem::new(role).with(
            Binding::FormButtonText.as_str(),
            Scalar::Text(text.to_owned()),
        )
    };
    let items: Vec<_> = form
        .elements
        .iter()
        .map(|element| match element {
            ActionElement::Button(button) => {
                let (path, file_system) = image_bindings(button.image.as_ref());
                text_item("button", &button.text)
                    .with(
                        Binding::FormButtonTexture.as_str(),
                        Scalar::Text(path.to_owned()),
                    )
                    .with(
                        Binding::FormButtonTextureFileSystem.as_str(),
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
        Binding::FormButtonContents.as_str(),
        Scalar::Json(serde_json::Value::Array(contents)),
    );
    data.set_collection("form_buttons", items);
}

fn custom_form_source(data: &mut DataSource, form: &CustomForm) {
    let (icon, file_system) = image_bindings(form.icon.as_ref());
    data.set_global(Binding::ServerIcon.as_str(), Scalar::Text(icon.to_owned()));
    data.set_global(
        Binding::ServerOutlineIcon.as_str(),
        Scalar::Text(icon.to_owned()),
    );
    data.set_global(
        Binding::ServerIconFileSystem.as_str(),
        Scalar::Text(file_system.to_owned()),
    );
    data.set_creation_value(
        Binding::TitleText.as_str(),
        Scalar::Text(form.title.clone()),
    );
    data.set_global(
        Binding::TitleText.as_str(),
        Scalar::Text(form.title.clone()),
    );
    data.set_global(
        Binding::CustomFormLength.as_str(),
        Scalar::Num(form.elements.len() as f64),
    );
    data.set_global(
        Binding::SubmitText.as_str(),
        Scalar::Text(form.submit_text.clone()),
    );
    data.set_global(
        Binding::SubmitButtonVisible.as_str(),
        Scalar::Bool(form.submit_visible),
    );
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
                        Binding::CustomMultiselectText.as_str(),
                        Binding::CustomMultiselectToggled.as_str(),
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
                    Binding::CustomRadioText.as_str(),
                    Binding::CustomRadioToggled.as_str(),
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
            CollectionItem::new("label").with(Binding::CustomText.as_str(), text(label))
        }
        CustomElement::Header { text: label } => {
            CollectionItem::new("header").with(Binding::CustomText.as_str(), text(label))
        }
        CustomElement::Divider => CollectionItem::new("divider"),
        CustomElement::Toggle {
            text: label,
            enabled,
            on,
            tooltip,
        } => CollectionItem::new("toggle")
            .with(Binding::CustomText.as_str(), text(label))
            .with(Binding::CustomToggleState.as_str(), Scalar::Bool(*on))
            .with(
                Binding::CustomToggleEnabled.as_str(),
                Scalar::Bool(*enabled),
            )
            .with(Binding::CustomTooltipText.as_str(), text(tooltip)),
        CustomElement::Slider {
            text: label,
            enabled,
            fraction,
            timeout,
            tooltip,
        } => CollectionItem::new("slider")
            .with(Binding::CustomSliderText.as_str(), text(label))
            .with(Binding::CustomSliderTextValue.as_str(), text(label))
            .with(
                Binding::CustomSliderValue.as_str(),
                Scalar::Num(fraction.clamp(0.0, 1.0)),
            )
            .with(Binding::CustomSliderTimeout.as_str(), Scalar::Num(*timeout))
            .with(
                Binding::CustomSliderEnabled.as_str(),
                Scalar::Bool(*enabled),
            )
            .with(Binding::CustomTooltipText.as_str(), text(tooltip)),
        CustomElement::StepSlider {
            text: label,
            enabled,
            steps,
            index,
            tooltip,
        } => CollectionItem::new("step_slider")
            .with(Binding::CustomSliderStepText.as_str(), text(label))
            .with(Binding::CustomSliderStepTextValue.as_str(), text(label))
            .with(
                Binding::CustomSliderStepValue.as_str(),
                Scalar::Num(*index as f64),
            )
            .with(
                Binding::CustomSliderSteps.as_str(),
                Scalar::Num((*steps).max(1) as f64),
            )
            .with(
                Binding::CustomSliderEnabled.as_str(),
                Scalar::Bool(*enabled),
            )
            .with(Binding::CustomTooltipText.as_str(), text(tooltip)),
        CustomElement::Dropdown {
            text: label,
            options,
            index,
            open,
            tooltip,
        } => CollectionItem::new("dropdown")
            .with(Binding::CustomText.as_str(), text(label))
            .with(
                Binding::DropdownOptionText.as_str(),
                text(options.get(*index as usize).map_or("", String::as_str)),
            )
            .with("#custom_dropdown", Scalar::Bool(*open))
            .with(
                Binding::CustomDropdownLength.as_str(),
                Scalar::Num(options.len() as f64),
            )
            .with(Binding::CustomToggleEnabled.as_str(), Scalar::Bool(true))
            .with(Binding::CustomTooltipText.as_str(), text(tooltip)),
        CustomElement::MultiSelect {
            text: label,
            options,
            open,
            tooltip,
            ..
        } => CollectionItem::new("multiselect")
            .with(Binding::CustomText.as_str(), text(label))
            .with(Binding::CustomMultiselect.as_str(), Scalar::Bool(*open))
            .with(
                Binding::CustomMultiselectLength.as_str(),
                Scalar::Num(options.len() as f64),
            )
            .with(Binding::CustomToggleEnabled.as_str(), Scalar::Bool(true))
            .with(Binding::CustomTooltipText.as_str(), text(tooltip)),
        CustomElement::Input {
            text: label,
            enabled,
            value,
            placeholder,
            tooltip,
        } => CollectionItem::new("input")
            .with(Binding::CustomText.as_str(), text(label))
            .with(Binding::CustomInputText.as_str(), text(value))
            .with(Binding::CustomPlaceholderText.as_str(), text(placeholder))
            .with(Binding::CustomInputEnabled.as_str(), Scalar::Bool(*enabled))
            .with(Binding::CustomTooltipText.as_str(), text(tooltip)),
    }
}

/// A [`ControlLibrary`] over the pack catalog: factory `control_ids` resolve as
/// standalone `namespace.name` templates in `context`.
pub struct CatalogLibrary<'a> {
    pub catalog: &'a Catalog,
    pub context: &'a Context,
}

impl<'a> CatalogLibrary<'a> {
    /// Restricts each factory prototype before its descendants are materialized.
    pub fn with_node_limit(self, limit: usize) -> BoundedCatalogLibrary<'a> {
        BoundedCatalogLibrary {
            library: self,
            node_limit: limit.clamp(1, crate::resolve::MAX_NODES),
        }
    }

    /// Borrows this catalog and context with a caller's prototype budget.
    fn bounded(&self, limit: usize) -> BoundedCatalogLibrary<'_> {
        CatalogLibrary {
            catalog: self.catalog,
            context: self.context,
        }
        .with_node_limit(limit)
    }
}

impl ControlLibrary for CatalogLibrary<'_> {
    fn resolve(&self, reference: &ControlRef) -> Option<ResolvedControl> {
        self.bounded(crate::resolve::MAX_NODES).resolve(reference)
    }

    fn resolve_with(
        &self,
        reference: &ControlRef,
        key: &str,
        vars: &dyn Fn() -> std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Option<ResolvedControl> {
        self.bounded(crate::resolve::MAX_NODES)
            .resolve_with(reference, key, vars)
    }
}

/// A catalog library with a smaller per-prototype resolution budget.
pub struct BoundedCatalogLibrary<'a> {
    library: CatalogLibrary<'a>,
    node_limit: usize,
}

impl BoundedCatalogLibrary<'_> {
    /// Resolves a prototype in the existing root scope under this library's cap.
    fn resolve_in(
        &self,
        reference: &ControlRef,
        root: &crate::env::Env,
    ) -> Option<ResolvedControl> {
        crate::Resolver::new(self.library.catalog)
            .with_limits(crate::resolve::MAX_DEPTH, self.node_limit)
            .resolve(&reference.namespace, &reference.name, root)
    }
}

impl ControlLibrary for BoundedCatalogLibrary<'_> {
    fn resolve(&self, reference: &ControlRef) -> Option<ResolvedControl> {
        self.resolve_in(
            reference,
            &self.library.context.root_env(self.library.catalog),
        )
    }

    fn resolve_with(
        &self,
        reference: &ControlRef,
        _key: &str,
        vars: &dyn Fn() -> std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Option<ResolvedControl> {
        let mut context = self.library.context.clone();
        for (name, value) in vars() {
            context = context.with_var(name.trim_start_matches('$'), value);
        }
        self.resolve_in(reference, &context.root_env(self.library.catalog))
    }
}

/// Factory and grid resolutions kept across binds of one catalog and context,
/// so a screen re-bound for changed data reuses its created controls' trees.
#[derive(Default)]
pub struct ResolveCache {
    memo: std::sync::Mutex<Option<ResolveMemo>>,
    node_limit: Option<usize>,
}

impl ResolveCache {
    /// Restricts prototype resolutions, discarding entries built under another cap.
    pub fn with_node_limit(mut self, limit: usize) -> Self {
        let limit = limit.clamp(1, crate::resolve::MAX_NODES);
        if self.node_limit() != limit {
            self.memo = std::sync::Mutex::default();
            self.node_limit = Some(limit);
        }
        self
    }

    /// The configured cap, or the ordinary resolved-tree maximum.
    fn node_limit(&self) -> usize {
        self.node_limit.unwrap_or(crate::resolve::MAX_NODES)
    }
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
        let library = self.library.bounded(self.cache.node_limit());
        let Ok(mut memo) = self.cache.memo.lock() else {
            return library.resolve_with(reference, key, vars).map(Arc::new);
        };
        let memo = memo.get_or_insert_with(|| ResolveMemo {
            trees: crate::lru::Lru::new(RESOLUTIONS),
            root: self.library.context.root_env(self.library.catalog),
        });
        if let Some(resolved) = memo.trees.get(&cache_key) {
            return resolved.clone();
        }
        let vars = vars();
        // A null `$var` reads as unset rather than masking a global, which a
        // scope over the shared root cannot express.
        let resolved = if vars.values().any(serde_json::Value::is_null) {
            library.resolve_with(reference, key, &|| vars.clone())
        } else {
            let mut root = memo.root.child();
            for (name, value) in vars {
                root.set(name.trim_start_matches('$'), value);
            }
            library.resolve_in(reference, &root.settle())
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
        let root = screen_content(catalog, &context)?;
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
                    nodes: output.nodes.into(),
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
            global_mapping(&laid, Button::MenuCancel.as_str()),
            find_rect(&laid, "root_panel"),
        )
    };
    FormRender {
        bound,
        nodes: nodes.into(),
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

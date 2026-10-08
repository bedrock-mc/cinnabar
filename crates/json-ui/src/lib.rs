//! Clean-room parser, resolver, and layout engine for vanilla Bedrock JSON-UI.
//!
//! The pipeline reads the on-disk `ui/*.json` (tolerant JSON5), applies `@base`
//! inheritance, substitutes `$var`/global references, evaluates `ignored` and
//! `variables[]` conditionals, and records factory mappings into a concrete
//! [`ResolvedControl`] tree. From there [`layout`] evaluates the size/offset
//! expressions ([`expr`]) against a virtual root size and positions every control,
//! and [`emit`] flattens the placed tree into layer-ordered draw commands,
//! nine-slicing sprites from their texture sidecars ([`sidecar`]). [`bind`] resolves
//! `#bindings` against a screen data source and expands factory collections and
//! grids, [`widgets`] drives the engine-owned control states from a caller-held
//! [`ViewState`], [`input`] reports interactive regions and mappings, [`pack`]
//! overlays server resource-pack ui over the vanilla catalog, and [`form`]
//! renders a decoded server form through its vanilla template.

mod anim;
mod bind;
mod catalog;
mod component;
mod context;
mod emit;
mod env;
mod expr;
mod form;
mod hud;
mod input;
mod json5;
mod label;
mod layout;
mod localize;
mod lru;
mod merge;
mod pack;
mod predicate;
mod resolve;
mod scene;
mod screens;
mod sidecar;
mod sprite;
mod state;
mod tree;
mod widgets;

#[cfg(test)]
mod allocation_count;

pub use anim::{
    AnimEvent, AnimGraph, AnimKind, AnimNode, Animated, Animator, ControlAnims, Easing, FlipWrite,
    NodeAnim, Written,
};
pub use bind::{
    BindState, CUSTOM_CONTROL_INSTANCE_KEY, CollectionItem, ControlLibrary, DataSource,
    EmptyLibrary, FactoryItem, bind, bind_incremental, bind_reporting, bind_shared, bind_stateful,
    rebind, scoped_key,
};
pub use catalog::{Catalog, LoadError, RawControl};
pub use component::{
    ButtonEvent, ButtonInput, CARET_BLINK_SECONDS, CARET_GLYPH, Components, Dispatch, Dispatcher,
    EditMeta, PointerInput, ScreenEvent, SelectionWheelMeta, SliderMeta, SoundMeta, TextEdit,
    TextType, ToggleManager, ToggleMeta, Widget,
};
pub use context::Context;
pub use emit::{
    Draw, DrawNode, RectOut, SpriteFilter, SpriteQuad, StateGate, TextAlign, UvRect, color_value,
    emit, emit_gated,
};
pub use env::Env;
pub use expr::{AxisContext, Length, Resolved, Term, Unit, length_from_value, parse_length};
pub use form::{
    ActionElement, ActionForm, ButtonImage, CachedLibrary, CatalogLibrary, CustomElement,
    CustomForm, FormButton, FormModel, FormRender, ModalForm, ResolveCache, bind_form,
    bind_form_over, form_context, form_data_source, form_factory_id, form_screen_cancel,
    form_template, render_bound, render_bound_cached, render_bound_gated, render_form,
    render_form_with,
};
pub use hud::{
    BossBar, CROSSHAIR_SCREEN, HUD_SCREEN, HudModel, HudSlot, HudTitle, Sidebar, Timed, hud_clocks,
    hud_context, hud_data_source,
};
pub use input::{
    ControlSound, HitKind, HitRegion, InputComponent, InputMode, InputModeCondition, Mapping,
    MappingScope, MappingType, focus_order, global_mapping, hit_regions, hit_test, region_rect,
    scroll_target,
};
pub use input::{
    CustomRoute, FOCUS_OVERRIDE_STOP, FocusContainer, FocusDirection, FocusMeta, FocusMove,
    NavigationMode, controller_direction_claimed, default_focus, navigate, next_in_order,
    set_focus,
};
pub use label::{LabelShape, TextOptions};
pub use layout::{
    LaidOut, LayoutEnv, MeasureCache, Rect, TextMeasure, TextureSource, layout, layout_with,
};
pub use localize::{localize_text, localize_text_prefix};
pub use predicate::{Bindings, Scalar};
pub use resolve::Resolver;
pub use scene::{SceneEntry, SceneStack, ScreenNav, ScreenSettings};
pub use screens::{
    ENGINE_SCREENS, ScreenRender, bind_screen, is_engine_screen, render_screen, resolve_screen,
    screen_settings,
};
pub use sidecar::{
    AsepriteFrame, NineSlice, TextureMeta, parse_aseprite_frames, parse_texture_meta,
};
pub use sprite::nine_slice;
pub use state::{FocusMemory, LayoutReport, ScrollMetrics, ScrollRetained, ViewState};
pub use tree::{ControlRef, Factory, Properties, ResolvedControl};
pub use widgets::{Draggable, ScrollMotion};

/// The outcome of a resolution: the tree (absent only when the reference is
/// unknown) and the diagnostics gathered along the way.
#[derive(Debug)]
pub struct Resolution {
    pub control: Option<ResolvedControl>,
    pub diagnostics: Vec<String>,
}

/// Resolve a `namespace.name` reference against `catalog` in `context`.
pub fn resolve(catalog: &Catalog, reference: &str, context: &Context) -> Resolution {
    resolve_in(catalog, reference, &context.root_env(catalog))
}

/// [`resolve`] in a root scope already built from a catalog and context.
fn resolve_in(catalog: &Catalog, reference: &str, root: &env::Env) -> Resolution {
    let mut resolver = Resolver::new(catalog);
    let control = match reference.split_once('.') {
        Some((namespace, name)) => {
            let resolved = resolver.resolve(namespace, name, root);
            if resolved.is_none() {
                resolver.note(format!("unknown control `{reference}`"));
            }
            resolved
        }
        None => {
            resolver.note(format!("reference `{reference}` is not `namespace.name`"));
            None
        }
    };
    Resolution {
        control,
        diagnostics: resolver.into_diagnostics(),
    }
}

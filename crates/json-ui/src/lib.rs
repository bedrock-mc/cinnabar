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

use std::collections::BTreeMap;

use serde_json::Value;

pub use anim::{
    AnimEvent, AnimGraph, AnimKind, AnimNode, Animated, Animator, ControlAnims, Easing, FlipWrite,
    NodeAnim, Written,
};
pub use bind::{
    BindState, CollectionItem, ControlLibrary, DataSource, EmptyLibrary, FactoryItem, bind,
    bind_reporting, bind_shared, bind_stateful, scoped_key,
};
pub use catalog::{Catalog, LoadError, RawControl};
pub use component::{
    ButtonEvent, ButtonInput, CARET_BLINK_SECONDS, CARET_GLYPH, Components, Dispatch, Dispatcher,
    EditMeta, PointerInput, ScreenEvent, SliderMeta, SoundMeta, TextEdit, TextType, ToggleManager,
    ToggleMeta, Widget,
};
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
pub use localize::localize_text;
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

/// Screen context: the compile-time flags (`$desktop_screen`, `$touch`, …) and any
/// extra variables that gate `ignored`/`variables[]` selection and `$var` values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Context {
    vars: BTreeMap<String, Value>,
}

impl Context {
    /// An empty context. As in the vanilla client, an unset `$flag` reads as its
    /// literal text: `ignored` keeps the control and a `requires` block applies,
    /// while inside an expression it is null.
    pub fn empty() -> Self {
        Self::default()
    }

    /// The desktop screen context used by this tranche's fixtures.
    pub fn desktop() -> Self {
        Self::empty()
            .with_flag("desktop_screen", true)
            .with_flag("pocket_screen", false)
            .with_flag("touch", false)
    }

    /// The desktop context plus the globals a retail, full-game, non-edu desktop
    /// client computes in code, false ones included.
    pub fn retail(macos: bool) -> Self {
        let platform: &[(&str, bool)] = &[
            ("win10_edition", !macos),
            ("microsoft_os", !macos),
            ("ms_platform", !macos),
            ("osx_edition", macos),
            ("apple_os", macos),
        ];
        let constant: &[(&str, bool)] = &[
            ("is_desktop", true),
            ("mouse", true),
            ("is_publish", true),
            ("test_infrastructure_disabled", true),
            ("new_video_settings", true),
            ("is_improve_input_response_platform_supported", true),
            ("is_xboxlive_enabled", true),
            ("is_realms_enabled", true),
            ("is_seeds_enabled", true),
            ("is_creative_enabled", true),
            ("is_multiplayer_enabled", true),
            ("is_packs_enabled", true),
            ("is_server_enabled", true),
            ("is_store_enabled", true),
            ("file_picking_supported", true),
            ("supports_clipboard_set", true),
            ("supports_add_friend", true),
            ("supports_xbl_achievements", true),
            // Channel flags: this is the release app, not Preview.
            ("pre_release", false),
            ("beta_build", false),
            ("is_preview_app", false),
            ("trial", false),
            ("education_edition", false),
            ("store_disabled", false),
            ("creator_build", false),
            ("pocket_edition", false),
            ("console_edition", false),
            ("is_console", false),
            ("game_pad", false),
            ("can_splitscreen", false),
            ("is_secondary_client", false),
            ("requires_xbl_signin_to_play", false),
            ("is_editor_mode_enabled", false),
            ("can_quit", true),
            ("world_archive_support", true),
            ("is_dynamic_textures_platform_supported", true),
            ("is_pregame", false),
            ("screen_transitions_enabled", false),
            ("use_normalized_font_size", false),
            ("image_picking_not_supported", false),
            ("vibration_supported", false),
            ("supports_share", false),
            ("hide_xbox_live_icon", false),
            ("disable_gamertag_controls", false),
            ("multiplayer_requires_live_gold", false),
            ("device_must_be_removed_for_xbl_signin", false),
            ("is_low_memory_device", false),
            ("ignore_3rd_party_servers", false),
            ("ignore_add_servers", false),
            ("is_on_3p_server", false),
            ("is_editor_playtest_roundtrip", false),
            ("edu_save_to_cloud_on", false),
            ("edu_save_to_cloud_general_toggle_on", false),
            ("built_with_ore_ui_docs_and_tests", false),
            // Other platforms and devices.
            ("build_platform_UWP", false),
            ("google_os", false),
            ("is_ios", false),
            ("is_android", false),
            ("is_chromebook", false),
            ("fire_tv", false),
            ("nx_os", false),
            ("is_ps4", false),
            ("is_ps5", false),
            ("xbox_one", false),
            ("thirdpartyconsole", false),
            ("is_settopbox", false),
            ("is_win10_arm", false),
            ("is_windows_10_mobile", false),
            ("is_mobile_vr", false),
            ("gear_vr", false),
            ("oculus_rift", false),
            ("psvr", false),
            ("is_holographic", false),
            ("supports_hand_controllers", false),
            ("is_living_room_mode", false),
            ("is_reality_mode", false),
        ];
        let context = platform
            .iter()
            .chain(constant)
            .fold(Self::desktop(), |context, (name, value)| {
                context.with_flag(name, *value)
            });
        // `SceneFactory::_createSafeZoneSizeVar` at the desktop defaults (safe
        // zone 1, screen position 0) sizes every buffer zero along its axis.
        let vertical = || serde_json::json!(["100%", 0]);
        let horizontal = || serde_json::json!([0, "100%"]);
        context
            .with_var("top_vertical_safezone_size", vertical())
            .with_var("bottom_vertical_safezone_size", vertical())
            .with_var("left_horizontal_safezone_size", horizontal())
            .with_var("right_horizontal_safezone_size", horizontal())
    }

    /// The variables set so far, keyed without `$`.
    pub fn vars(&self) -> &BTreeMap<String, Value> {
        &self.vars
    }

    /// Set a boolean flag (stored under `name`, without a `$`).
    pub fn with_flag(mut self, name: &str, value: bool) -> Self {
        self.vars.insert(name.to_owned(), Value::Bool(value));
        self
    }

    /// Set an arbitrary variable value.
    pub fn with_var(mut self, name: &str, value: Value) -> Self {
        self.vars.insert(name.to_owned(), value);
        self
    }

    fn root_env(&self, catalog: &Catalog) -> Env {
        let mut env = Env::new();
        for (name, value) in catalog.globals() {
            env.set(name.clone(), value.clone());
        }
        for (name, value) in &self.vars {
            env.set(name.clone(), value.clone());
        }
        env
    }
}

/// The outcome of a resolution: the tree (absent only when the reference is
/// unknown) and the diagnostics gathered along the way.
#[derive(Debug)]
pub struct Resolution {
    pub control: Option<ResolvedControl>,
    pub diagnostics: Vec<String>,
}

/// Resolve a `namespace.name` reference against `catalog` in `context`.
pub fn resolve(catalog: &Catalog, reference: &str, context: &Context) -> Resolution {
    let root = context.root_env(catalog);
    let mut resolver = Resolver::new(catalog);
    let control = match reference.split_once('.') {
        Some((namespace, name)) => {
            let resolved = resolver.resolve(namespace, name, &root);
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

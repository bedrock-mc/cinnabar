//! Container screens against the real vanilla templates. The `.local` pack is
//! gitignored, so reference tests name missing fixtures and skip when absent.

mod support;

use json_ui::{
    Catalog, CollectionItem, Context, DataSource, Draw, LayoutEnv, Scalar, TextMeasure,
    TextureMeta, TextureSource, ViewState, render_screen,
};

struct ZeroText;
impl TextMeasure for ZeroText {
    fn extent(&self, _text: &str) -> [f64; 2] {
        [0.0, 0.0]
    }
}

struct NoTextures;
impl TextureSource for NoTextures {
    fn texture(&self, _path: &str) -> Option<TextureMeta> {
        None
    }
}

fn env() -> LayoutEnv<'static> {
    LayoutEnv {
        text: &ZeroText,
        textures: &NoTextures,
    }
}

fn catalog() -> Option<Catalog> {
    let dir = support::vanilla_pack().join("ui");
    dir.is_dir()
        .then(|| Catalog::load_dir(&dir).expect("index files load"))
}

fn items(count: usize, icon: bool) -> Vec<CollectionItem> {
    (0..count)
        .map(|index| {
            let item = CollectionItem::default().with(
                "#inventory_stack_count",
                Scalar::Text(if index == 0 {
                    "5".into()
                } else {
                    String::new()
                }),
            );
            if icon && index == 0 {
                item.with("#item_renderer_data", Scalar::Num(0.0))
            } else {
                item
            }
        })
        .collect()
}

fn chest_data() -> DataSource {
    let mut data = DataSource::new();
    // A screen controller answers the bindings it lacks with false.
    data.set_strict(true);
    data.set_collection("container_items", items(27, true));
    data.set_collection("inventory_items", items(27, false));
    data.set_collection("hotbar_items", items(9, false));
    data
}

#[test]
fn small_chest_exposes_every_slot_by_collection() {
    let Some(catalog) = catalog() else {
        return;
    };
    let render = render_screen(
        "chest.small_chest_screen",
        &catalog,
        &Context::desktop(),
        &chest_data(),
        [480.0, 270.0],
        &env(),
        &ViewState::default(),
    )
    .expect("small chest renders");
    let slots = |collection: &str| {
        let mut indices: Vec<usize> = render
            .hits
            .iter()
            .filter(|hit| hit.collection.as_deref() == Some(collection))
            .filter_map(|hit| hit.collection_index)
            .collect();
        indices.sort_unstable();
        indices.dedup();
        indices.len()
    };
    assert_eq!(slots("container_items"), 27);
    assert_eq!(slots("inventory_items"), 27);
    assert_eq!(slots("hotbar_items"), 9);
    assert!(
        render.root_panel.is_some(),
        "the panel bounds outside clicks"
    );
    let renderers: Vec<&str> = render
        .nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Custom { renderer, data } if data.contains_key("#item_renderer_data") => {
                Some(renderer.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        renderers,
        ["inventory_item_renderer"],
        "only the stocked cell draws an item"
    );
    assert_eq!(render.cancel_target.as_deref(), Some("button.menu_exit"));
}

// Vanilla screen roots carry their pack-declared settings over the parser defaults.
#[test]
fn vanilla_screen_settings_come_from_the_pack() {
    let Some(catalog) = catalog() else {
        return;
    };
    let context = Context::desktop();
    let hud = json_ui::screen_settings("hud.hud_screen", &catalog, &context).unwrap();
    assert!(!hud.absorbs_input && !hud.is_showing_menu && hud.should_steal_mouse);
    assert!(hud.low_frequency_rendering && hud.render_only_when_topmost);
    let toast = json_ui::screen_settings("toast_screen.toast_screen", &catalog, &context).unwrap();
    assert!(toast.always_accepts_input && toast.screen_draws_last && toast.screen_not_flushable);
    assert!(!toast.render_only_when_topmost && toast.is_modal);
    let furnace = json_ui::screen_settings("furnace.furnace_screen", &catalog, &context).unwrap();
    assert!(furnace.close_on_player_hurt && furnace.absorbs_input);
    let pause = json_ui::screen_settings("pause.pause_screen", &catalog, &context).unwrap();
    assert!(pause.cache_screen && pause.is_showing_menu && !pause.should_steal_mouse);
    let dialog = json_ui::screen_settings("common.render_below_base_screen", &catalog, &context);
    assert!(dialog.unwrap().force_render_below);
    assert!(json_ui::screen_settings("hud.hud_content", &catalog, &context).is_none());
}

#[test]
fn scene_flags_follow_inheritance_and_context() {
    let mut catalog = Catalog::default();
    catalog.overlay_text(
        "ui/policy.json",
        r#"{
      "namespace": "policy",
      "base": {"type": "screen", "absorbs_input": false,
        "render_game_behind": "$world", "render_only_when_topmost": false},
      "child@base": {},
      "opaque@base": {"absorbs_input": true, "render_game_behind": false},
      "defaults": {"type": "screen"}
    }"#,
    );
    let context = Context::default().with_flag("world", true);
    let settings = |name| {
        json_ui::ScreenSettings::from_root(
            &json_ui::resolve(&catalog, name, &context).control.unwrap(),
        )
    };
    let child = settings("policy.child");
    assert!(!child.absorbs_input);
    assert!(child.render_game_behind);
    assert!(!child.render_only_when_topmost);
    assert!(child.renders(false));
    let opaque = settings("policy.opaque");
    assert!(opaque.absorbs_input);
    assert!(!opaque.render_game_behind);
    assert!(!settings("policy.defaults").renders(false));
    assert!(settings("policy.defaults").renders(true));
    assert_eq!(
        settings("policy.defaults"),
        json_ui::ScreenSettings::default()
    );
}

// Each server-info edit box reports its vanilla text box name, which picks the field it edits.
#[test]
fn add_server_edit_boxes_report_their_text_box_names() {
    let Some(catalog) = catalog() else {
        return;
    };
    let render = render_screen(
        "add_external_server.add_external_server_screen_new",
        &catalog,
        &Context::desktop(),
        &DataSource::default(),
        [480.0, 270.0],
        &env(),
        &ViewState::default(),
    )
    .expect("add server renders");
    let mut names: Vec<_> = render
        .hits
        .iter()
        .filter(|hit| hit.kind == json_ui::HitKind::EditBox)
        .map(|hit| hit.control_name.clone())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["#ip_text_box", "#name_text_box", "#port_text_box"].map(|n| Some(n.to_owned()))
    );
}

/// Finds a visible control's authored geometry, including offscreen descendants.
fn visible_rect(tree: &json_ui::LaidOut<'_>, name: &str) -> Option<json_ui::Rect> {
    if !tree.visible {
        return None;
    }
    if tree.control.name == name {
        return Some(tree.rect);
    }
    tree.children
        .iter()
        .find_map(|child| visible_rect(child, name))
}

// The real pack chooses a compact +/- expander; its grid must follow the controller state.
#[test]
fn vanilla_video_graphics_expander_uses_pack_geometry_and_reveals_options() {
    let Some(catalog) = catalog() else {
        return;
    };
    let context = Context::desktop();
    let control = json_ui::resolve(&catalog, "general_section.video_section", &context)
        .control
        .unwrap();
    let library = json_ui::CatalogLibrary {
        catalog: &catalog,
        context: &context,
    };
    for expanded in [false, true] {
        let mut data = DataSource::new();
        data.set_strict(true);
        for (name, value) in [
            ("#advanced_graphics_options_button_visible", true),
            ("#advanced_graphics_options_grid_visible", expanded),
            ("#max_framerate_slider_visible", true),
            ("#graphics_mode_dropdown_enabled", true),
        ] {
            data.set_global(name, Scalar::Bool(value));
        }
        data.set_global(
            "#graphics_mode_toggle_label",
            Scalar::Text("Simple Graphics Options".into()),
        );
        let bound = json_ui::bind(&control, &data, &library);
        let layout = json_ui::layout(&bound, [480.0, 270.0], &env());
        let button = visible_rect(&layout, "advanced_graphics_options_button").unwrap();
        assert_eq!(button.h, 20.0);
        assert_eq!(visible_rect(&layout, "plus_panel").is_some(), !expanded);
        assert_eq!(visible_rect(&layout, "minus_panel").is_some(), expanded);
        assert_eq!(
            visible_rect(&layout, "advanced_graphics_options_section").is_some(),
            expanded
        );
    }
}

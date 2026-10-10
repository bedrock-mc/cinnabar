//! `player-mod` through a real component: `examples/mods/screen-probe`, built for wasm32 and
//! packaged with its `mod.toml`, reports what each callback saw as bound values.

use super::*;
use crate::DataSource;
use server_experience::{
    screen::{GuiSize, Rect, ScreenLayout, Value},
    session_data::{
        Ingredient, IngredientKind, Item, ItemKey, Recipe, RecipeCategory, SessionData, Stack,
    },
};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::OnceLock};

const PROBE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/mods/screen-probe"
);

/// The probe's component, built once per test process into a target directory of its own.
fn probe_component() -> &'static [u8] {
    static COMPONENT: OnceLock<Vec<u8>> = OnceLock::new();
    COMPONENT.get_or_init(|| {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        env!("CARGO_MANIFEST_DIR").hash(&mut hasher);
        let exe = std::env::current_exe().unwrap();
        let target = exe
            .ancestors()
            .nth(3)
            .unwrap()
            .join(format!("mod-host-mods-{:016x}", hasher.finish()));
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let output = std::process::Command::new(cargo)
            .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .args(["build", "--locked", "--target", "wasm32-unknown-unknown"])
            .args(["-p", "screen-probe-mod", "--target-dir"])
            .arg(&target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "building the screen probe failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let module =
            std::fs::read(target.join("wasm32-unknown-unknown/debug/screen_probe_mod.wasm"))
                .unwrap();
        wit_component::ComponentEncoder::default()
            .module(&module)
            .unwrap()
            .validate(true)
            .encode()
            .unwrap()
    })
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Writes the probe's package with `edit` applied to its manifest text, hashing every file.
fn write_probe(dir: &Path, edit: impl Fn(&str) -> String) {
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    let component = probe_component();
    std::fs::write(dir.join("mod.wasm"), component).unwrap();
    let mut files = format!("\"mod.wasm\" = \"{}\"\n", sha(component));
    for name in ["overlay", "view"] {
        let bytes = std::fs::read(Path::new(PROBE).join(format!("ui/{name}.json"))).unwrap();
        std::fs::write(dir.join(format!("ui/{name}.json")), &bytes).unwrap();
        files.push_str(&format!("\"ui/{name}.json\" = \"{}\"\n", sha(&bytes)));
    }
    let manifest = std::fs::read_to_string(Path::new(PROBE).join("mod.toml")).unwrap();
    std::fs::write(dir.join("mod.toml"), edit(&manifest) + &files).unwrap();
}

fn probe_with(edit: impl Fn(&str) -> String) -> (tempfile::TempDir, ModHost) {
    let dir = tempfile::tempdir().unwrap();
    write_probe(dir.path(), edit);
    let host = ModHost::load_package(dir.path(), ModGrants::default()).unwrap();
    (dir, host)
}

fn probe() -> (tempfile::TempDir, ModHost) {
    probe_with(str::to_owned)
}

fn value<'a>(host: &'a ModHost, name: &str) -> Option<&'a Value> {
    host.screens().data.values.get(name)
}

fn text(host: &ModHost, name: &str) -> String {
    match value(host, name) {
        Some(Value::Text(text)) => text.clone(),
        other => panic!("{name}: {other:?}"),
    }
}

fn layout() -> ScreenLayout {
    ScreenLayout {
        screen: "crafting.inventory_screen".into(),
        size: GuiSize {
            width: 480.0,
            height: 270.0,
            scale: 2.0,
        },
        gui: Rect {
            x: 152.0,
            y: 52.0,
            width: 176.0,
            height: 166.0,
        },
        exclusions: vec![Rect::default()],
        view: None,
    }
}

fn key(identifier: &str) -> ItemKey {
    ItemKey {
        identifier: identifier.into(),
        aux: 0,
    }
}

fn session(items: usize, revision: u64) -> Arc<SessionData> {
    session_sized(items, 3, revision)
}

/// `items` items and `recipes` three-by-three crafting recipes, named like vanilla's.
fn session_sized(items: usize, recipes: usize, revision: u64) -> Arc<SessionData> {
    let item = |index: usize| Item {
        key: key(&format!("minecraft:item_{index}")),
        icon: SessionData::icon_key(index as i32, 0),
        name: format!("Item {index}"),
        group: None,
        category: None,
        max_stack: 64,
        tags: Vec::new(),
    };
    let recipe = Recipe {
        network_id: 7,
        category: RecipeCategory::Crafting,
        shapeless: false,
        width: 3,
        height: 3,
        ingredients: (0..9)
            .map(|cell| match cell % 3 {
                0 => Some(Ingredient {
                    kind: IngredientKind::Tag("minecraft:planks".into()),
                    count: 1,
                }),
                1 => Some(Ingredient {
                    kind: IngredientKind::Item(ItemKey {
                        identifier: "minecraft:white_wool".into(),
                        aux: 0,
                    }),
                    count: 1,
                }),
                _ => None,
            })
            .collect(),
        outputs: vec![Stack {
            key: key("minecraft:bed"),
            icon: SessionData::icon_key(9, 0),
            count: 1,
        }],
    };
    Arc::new(SessionData {
        item_revision: revision,
        items: (0..items).map(item).collect(),
        recipe_revision: revision,
        recipes: vec![recipe; recipes].into(),
        ..SessionData::default()
    })
}

fn action(id: &str) -> ModEvent {
    ModEvent::Action {
        id: id.into(),
        index: Some(3),
    }
}

#[test]
fn a_package_initializes_its_overlay_and_reads_the_session_on_data_changed() {
    let (_dir, mut host) = probe();
    assert!(host.has_events());
    assert_eq!(host.screens().overlay.as_deref(), Some("ui/overlay.json"));
    assert_eq!(value(&host, "#items"), Some(&Value::Integer(0)));
    host.set_session(session(600, 1)).unwrap();
    assert_eq!(value(&host, "#items"), Some(&Value::Integer(600)));
    assert_eq!(
        value(&host, "#page"),
        Some(&Value::Integer(
            server_experience::session_data::MAX_PAGE as i64
        ))
    );
    assert_eq!(text(&host, "#first"), "Item 0");
    assert_eq!(value(&host, "#recipes"), Some(&Value::Integer(3)));
    assert_eq!(value(&host, "#first_cells"), Some(&Value::Integer(9)));
    // The same revisions deliver nothing new.
    let revision = host.screens().data.revision;
    host.set_session(session(600, 1)).unwrap();
    assert_eq!(host.screens().data.revision, revision);
}

#[test]
fn session_reads_without_their_permission_are_denied() {
    let (_dir, mut host) = probe_with(|manifest| {
        manifest.replace(
            "permissions = [\"screen\", \"items\", \"recipes\", \"keys\"]",
            "permissions = [\"screen\"]",
        )
    });
    host.set_session(session(5, 1)).unwrap();
    assert!(text(&host, "#items_error").contains("denied"));
    assert!(text(&host, "#recipes_error").contains("denied"));
    assert!(host.is_active());
    let key = ModEvent::Key {
        id: "probe.show".into(),
        hovered: None,
        row: None,
    };
    assert!(host.dispatch(vec![key]).is_err());
}

#[test]
fn layout_changes_coalesce_and_the_view_needs_an_open_container() {
    let (_dir, mut host) = probe();
    host.dispatch(vec![action("probe.view")]).unwrap();
    assert!(text(&host, "#action").contains("Err"));
    assert_eq!(host.screens().view, None);
    let mut other = layout();
    other.screen = "chest.small_chest_screen".into();
    host.dispatch(vec![
        ModEvent::ScreenChanged(Some(other)),
        ModEvent::ScreenChanged(Some(layout())),
        action("probe.view"),
    ])
    .unwrap();
    assert_eq!(text(&host, "#screen"), "crafting.inventory_screen");
    assert_eq!(
        value(&host, "#gui"),
        Some(&Value::Numbers(vec![152.0, 52.0, 176.0]))
    );
    assert_eq!(value(&host, "#exclusions"), Some(&Value::Integer(1)));
    assert_eq!(value(&host, "#layout_open"), Some(&Value::Bool(true)));
    assert_eq!(host.screens().view.as_deref(), Some("ui/view.json"));
    // The host closing the view tells the mod, once.
    host.close_view().unwrap();
    assert_eq!(host.screens().view, None);
    assert_eq!(value(&host, "#view_closed"), Some(&Value::Integer(1)));
    host.close_view().unwrap();
    assert_eq!(value(&host, "#view_closed"), Some(&Value::Integer(1)));
    host.dispatch(vec![action("probe.view")]).unwrap();
    host.dispatch(vec![ModEvent::ScreenChanged(None)]).unwrap();
    assert_eq!(value(&host, "#view_closed"), Some(&Value::Integer(2)));
    assert_eq!(host.screens().view, None);
    assert_eq!(value(&host, "#layout_open"), Some(&Value::Bool(false)));
}

#[test]
fn input_events_reach_their_callbacks() {
    let (_dir, mut host) = probe();
    host.dispatch(vec![
        ModEvent::SecondaryAction {
            id: "probe.close".into(),
            index: Some(4),
        },
        ModEvent::Scrolled {
            delta: -2.0,
            x: 400.0,
            y: 30.0,
            modifiers: KeyModifiers {
                shift: true,
                ..KeyModifiers::default()
            },
        },
        ModEvent::TextChanged {
            control: "probe.search".into(),
            text: "dir".into(),
        },
        ModEvent::TextChanged {
            control: "probe.search".into(),
            text: "dirt".into(),
        },
        ModEvent::Key {
            id: "probe.show".into(),
            hovered: Some(Stack {
                key: key("minecraft:dirt"),
                icon: SessionData::icon_key(3, 0),
                count: 12,
            }),
            row: Some(("items".into(), 5)),
        },
    ])
    .unwrap();
    assert_eq!(text(&host, "#secondary"), "probe.close:Some(4)");
    assert_eq!(
        value(&host, "#scrolled"),
        Some(&Value::Numbers(vec![-2.0, 400.0, 30.0]))
    );
    assert_eq!(
        value(&host, "#scroll_modifiers"),
        Some(&Value::Numbers(vec![0.0, 1.0, 0.0]))
    );
    assert_eq!(text(&host, "#search"), "probe.search=dirt");
    assert_eq!(
        text(&host, "#key"),
        "probe.show:minecraft:dirt@0x12#196608:Some((\"items\", 5))"
    );
}

#[test]
fn undeclared_events_are_refused_without_quarantine() {
    let (_dir, mut host) = probe();
    assert!(host.dispatch(vec![action("probe.other")]).is_err());
    let key = ModEvent::Key {
        id: "probe.unknown".into(),
        hovered: None,
        row: None,
    };
    assert!(host.dispatch(vec![key]).is_err());
    assert!(host.is_active());
    assert!(host.screens().overlay.is_some());
}

#[test]
fn a_trap_in_key_quarantines_and_removes_the_overlay() {
    let (_dir, mut host) = probe();
    host.dispatch(vec![
        ModEvent::ScreenChanged(Some(layout())),
        action("probe.view"),
    ])
    .unwrap();
    let trap = ModEvent::Key {
        id: "probe.trap".into(),
        hovered: None,
        row: None,
    };
    assert!(host.dispatch(vec![trap]).is_err());
    assert!(!host.is_active());
    assert_eq!(host.screens().overlay, None);
    assert_eq!(host.screens().view, None);
    assert!(host.screens().data.values.is_empty());
    host.dispatch(vec![action("probe.view")]).unwrap();
    assert_eq!(host.screens().view, None);
}

#[test]
fn fuel_stops_a_loop_in_text_changed() {
    let (_dir, mut host) = probe();
    let looping = ModEvent::TextChanged {
        control: "probe.search".into(),
        text: "loop".into(),
    };
    let error = host.dispatch(vec![looping]).unwrap_err();
    assert!(format!("{error:#}").contains("fuel"), "{error:#}");
    assert!(!host.is_active());
    assert_eq!(host.screens().overlay, None);
}

#[test]
fn a_collection_flood_is_refused_within_the_output_budget() {
    let (_dir, mut host) = probe();
    host.dispatch(vec![action("probe.flood")]).unwrap();
    assert!(text(&host, "#action").contains("host output budget exceeded"));
    assert!(host.is_active());
    let bytes: usize = host
        .screens()
        .data
        .collections
        .values()
        .map(|rows| serde_json::to_vec(&rows[..]).unwrap().len())
        .sum();
    assert!(
        bytes <= server_experience::policy::MAX_HOST_OUTPUT,
        "{bytes}"
    );
}

#[test]
fn a_changed_package_reloads_with_the_current_layout_and_session() {
    let (dir, mut host) = probe();
    host.set_session(session(4, 1)).unwrap();
    host.dispatch(vec![ModEvent::ScreenChanged(Some(layout()))])
        .unwrap();
    assert!(!host.reload_if_changed().unwrap());
    write_probe(dir.path(), |manifest| manifest.replace("0.1.0", "0.1.1"));
    assert!(host.reload_if_changed().unwrap());
    assert_eq!(text(&host, "#screen"), "crafting.inventory_screen");
    assert_eq!(value(&host, "#items"), Some(&Value::Integer(4)));
    std::fs::write(dir.path().join("ui/view.json"), "{}").unwrap();
    assert!(host.reload_if_changed().is_err());
    assert!(host.is_active());
    assert_eq!(host.screens().overlay.as_deref(), Some("ui/overlay.json"));
}

#[test]
fn coalescing_keeps_the_latest_layout_and_text_per_box() {
    let text = |control: &str, text: &str| ModEvent::TextChanged {
        control: control.into(),
        text: text.into(),
    };
    let events = ModEvent::coalesce(vec![
        text("a", "1"),
        ModEvent::ScreenChanged(None),
        ModEvent::DataChanged(vec![DataSource::Recipes]),
        text("b", "x"),
        text("a", "2"),
        ModEvent::ScreenChanged(Some(layout())),
        ModEvent::DataChanged(vec![DataSource::Items, DataSource::Recipes]),
    ]);
    assert_eq!(
        events,
        vec![
            ModEvent::ScreenChanged(Some(layout())),
            ModEvent::DataChanged(vec![DataSource::Items, DataSource::Recipes]),
            text("b", "x"),
            text("a", "2"),
        ]
    );
}

/// Twice vanilla's session at the target (about 1,800 items and 2,000 recipes) loads through
/// `data-changed` on the load budget; the same reads in an ordinary event exceed
/// `CALLBACK_FUEL` and trap.
#[test]
fn a_twice_vanilla_session_loads_but_an_event_may_not_spend_that_much() {
    use server_experience::runtime::{CALLBACK_FUEL, LOAD_FUEL};
    let (_dir, mut host) = probe();
    host.set_session(session_sized(4_000, 3_000, 1)).unwrap();
    assert_eq!(value(&host, "#items_read"), Some(&Value::Integer(4_000)));
    assert_eq!(value(&host, "#recipes_read"), Some(&Value::Integer(3_000)));
    let used = host.last_fuel_used();
    eprintln!("twice-vanilla data-changed used {used} fuel");
    assert!(used > CALLBACK_FUEL && used < LOAD_FUEL, "{used}");
    let error = host.dispatch(vec![action("probe.read_all")]).unwrap_err();
    assert!(format!("{error:#}").contains("fuel"), "{error:#}");
    assert!(!host.is_active());
}

#[test]
fn a_package_in_a_set_needs_both_the_manifest_ask_and_the_loader_grant() {
    let dir = tempfile::tempdir().unwrap();
    write_probe(dir.path(), str::to_owned);
    let only_screen = ModGrants {
        screen: true,
        ..ModGrants::default()
    };
    let mut host = ModHost::load_package_with_grants(dir.path(), only_screen).unwrap();
    assert_eq!(host.screens().overlay.as_deref(), Some("ui/overlay.json"));
    host.set_session(session(5, 1)).unwrap();
    assert!(text(&host, "#items_error").contains("denied"));
    let unasked = tempfile::tempdir().unwrap();
    write_probe(unasked.path(), |manifest| {
        manifest.replace(
            "permissions = [\"screen\", \"items\", \"recipes\", \"keys\"]",
            "permissions = [\"items\"]",
        )
    });
    let all = ModGrants {
        screen: true,
        items: true,
        recipes: true,
        keys: true,
        ..ModGrants::default()
    };
    let host = ModHost::load_package_with_grants(unasked.path(), all).unwrap();
    assert!(!host.grants().screen);
    assert!(host.grants().items);
}

#[test]
fn a_package_keeps_its_settings_beside_its_directory() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("bei-0.1.0");
    write_probe(&dir, str::to_owned);
    let grants = ModGrants {
        settings: true,
        ..ModGrants::default()
    };
    let host = ModHost::load_package(&dir, grants).unwrap();
    let path = host.settings_path().unwrap();
    assert_eq!(path.file_name().unwrap(), "bei-0.1.0.settings.json");
    assert_eq!(
        path.parent().unwrap().canonicalize().unwrap(),
        root.path().canonicalize().unwrap()
    );
}

#[test]
fn an_extension_init_stays_on_the_frame_budget() {
    // About half a million instructions: within `LOAD_FUEL`, beyond `FRAME_FUEL`.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("busy.wat");
    std::fs::write(
        &path,
        r#"(component
            (core module $m
                (func (export "init") (local $n i32)
                    (local.set $n (i32.const 100000))
                    (loop $l
                        (local.set $n (i32.sub (local.get $n) (i32.const 1)))
                        (br_if $l (local.get $n))))
                (func (export "frame")))
            (core instance $i (instantiate $m))
            (func (export "init") (canon lift (core func $i "init")))
            (func (export "frame") (canon lift (core func $i "frame"))))"#,
    )
    .unwrap();
    assert!(ModHost::load(&path).is_err());
    let (_dir, host) = probe();
    assert!(host.last_fuel_used() <= server_experience::runtime::LOAD_FUEL);
}

/// A component exporting `init`, `frame` and the given event exports, lifted from `$body`
/// core functions named after them.
fn partial_component(dir: &Path, exports: &[(&str, &str, &str)]) -> std::path::PathBuf {
    let core: String = exports
        .iter()
        .map(|(name, params, body)| format!("(func (export \"{name}\") {params} {body})"))
        .collect();
    let lifted: String = exports
        .iter()
        .map(|(name, params, _)| {
            let params = params.replace("(param i32)", "(param \"x\" u32)");
            format!("(func (export \"{name}\") {params} (canon lift (core func $i \"{name}\")))")
        })
        .collect();
    let path = dir.join("partial.wat");
    std::fs::write(
        &path,
        format!(
            r#"(component
                (core module $m (func (export "init")) (func (export "frame")) {core})
                (core instance $i (instantiate $m))
                (func (export "init") (canon lift (core func $i "init")))
                (func (export "frame") (canon lift (core func $i "frame")))
                {lifted})"#
        ),
    )
    .unwrap();
    path
}

#[test]
fn a_component_exporting_some_events_gets_only_those() {
    let dir = tempfile::tempdir().unwrap();
    let path = partial_component(dir.path(), &[("view-closed", "", "unreachable")]);
    let mut host = ModHost::load(&path).unwrap();
    assert!(host.has_events());
    // Not exported: nothing is called.
    host.dispatch(vec![ModEvent::ScreenChanged(Some(layout()))])
        .unwrap();
    host.dispatch(vec![ModEvent::DataChanged(vec![DataSource::Items])])
        .unwrap();
    assert!(host.is_active());
    // Exported: called, and its trap quarantines.
    assert!(host.dispatch(vec![ModEvent::ViewClosed]).is_err());
    assert!(!host.is_active());
}

#[test]
fn an_event_export_of_another_signature_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = partial_component(dir.path(), &[("view-closed", "(param i32)", "")]);
    let error = ModHost::load(&path).err().expect("refused");
    assert!(format!("{error:#}").contains("view-closed"), "{error:#}");
}

#[test]
fn data_changed_names_what_changed() {
    let (_dir, mut host) = probe();
    host.set_session(session(5, 1)).unwrap();
    assert_eq!(text(&host, "#data_sources"), "items,recipes");
    let mut recipes_only = (*session(5, 1)).clone();
    recipes_only.recipe_revision = 2;
    host.set_session(Arc::new(recipes_only)).unwrap();
    assert_eq!(text(&host, "#data_sources"), "recipes");
}

#[test]
fn package_reload_rechecks_manifest_permissions_and_preserves_loader_policy() {
    for selected_alone in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        write_probe(dir.path(), str::to_owned);
        let grants = ModGrants {
            screen: true,
            recipes: true,
            keys: true,
            ..ModGrants::default()
        };
        let mut host = if selected_alone {
            ModHost::load_package(dir.path(), ModGrants::default()).unwrap()
        } else {
            ModHost::load_package_with_grants(dir.path(), grants).unwrap()
        };
        assert_eq!(host.grants().items, selected_alone);
        write_probe(dir.path(), |manifest| {
            manifest.replace("permissions = [", "permissions = [] # [")
        });
        assert!(host.reload_if_changed().unwrap());
        assert!(
            !host.grants().screen
                && !host.grants().items
                && !host.grants().recipes
                && !host.grants().keys
        );
        assert!(host.screens().overlay.is_none());
        assert!(
            host.dispatch(vec![ModEvent::Key {
                id: "probe.show".into(),
                hovered: None,
                row: None
            }])
            .is_err()
        );
        write_probe(dir.path(), str::to_owned);
        assert!(host.reload_if_changed().unwrap());
        assert!(host.grants().screen && host.grants().recipes && host.grants().keys);
        assert_eq!(host.grants().items, selected_alone);
        host.set_session(session(5, 1)).unwrap();
        if selected_alone {
            assert_eq!(value(&host, "#items"), Some(&Value::Integer(5)));
        } else {
            assert!(text(&host, "#items_error").contains("denied"));
        }
    }
}

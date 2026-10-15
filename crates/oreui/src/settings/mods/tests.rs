use {
    super::*,
    crate::paint::Canvas,
    crate::review_tests::paint,
    launcher::menu::MenuView,
    launcher::mods::{ActiveMod, Library, Permission},
    std::{collections::HashMap, sync::Arc},
    ui::IconRef,
};

/// Supplies one card with a declared camera permission for interaction assertions.
fn installed(id: u64, name: &str) -> InstalledMod {
    InstalledMod {
        id,
        file: format!("{name}.wasm"),
        bytes: 2048,
        permissions: vec![Permission {
            name: "camera".into(),
            description: "Allows camera output.".into(),
            enabled: false,
            requested: Some(true),
        }],
        metadata: Default::default(),
    }
}

/// Paints the page and exposes its visible text and identity-scoped hit actions.
fn render(snapshot: Snapshot) -> (Vec<MenuAction>, Vec<String>) {
    let (hits, labels, _) = render_with(snapshot, &HashMap::new());
    (hits, labels)
}

/// Paints the page over ready menu artwork, also returning its nodes.
fn render_with(
    snapshot: Snapshot,
    artwork: &HashMap<String, IconRef>,
) -> (Vec<MenuAction>, Vec<String>, Vec<ui::UiNode>) {
    let mut view = MenuView::new(true, "Player".into());
    view.mods = Arc::new(snapshot);
    let (mut nodes, mut next, mut layouts) =
        (Vec::new(), 1, ui::TextLayoutCache::new(128, 1024 * 1024));
    let font = ui_engine::test_support::fixture_font();
    let metrics = ui_engine::text_metrics::TextMetrics::for_viewport(
        [1280, 720],
        ui::DpiScale::new(1.0).unwrap(),
        Some(2),
    );
    let hits = {
        let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
        canvas.artwork = Some(artwork);
        let mut content = Content {
            canvas: &mut canvas,
            view: &view,
            span: [100.0, 900.0],
            column_width: 800.0,
            y: 100.0,
            translate: &|_| None,
            nested: false,
        };
        draw(&mut content).unwrap();
        std::mem::take(&mut canvas.hits)
    };
    let labels = nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. } => Some(
                layout
                    .glyphs()
                    .iter()
                    .map(|glyph| glyph.codepoint)
                    .collect(),
            ),
            _ => None,
        })
        .collect();
    (
        hits.into_iter().map(|(action, _)| action).collect(),
        labels,
        nodes,
    )
}

/// Every native build exposes mod import without a feature availability branch.
#[test]
fn empty_library_offers_import() {
    let (hits, labels) = render(Snapshot::default());
    assert!(labels.iter().any(|label| label == "No mods installed"));
    assert!(hits.contains(&command(Action::Import)));
}

#[test]
fn the_enabled_mod_is_the_active_card_with_its_status_and_controls() {
    let (first, second) = (installed(1, "zoom"), installed(2, "minimap"));
    let snapshot = Snapshot {
        library: Library {
            installed: vec![first.clone(), second.clone()],
            active: vec![ActiveMod {
                id: second.id,
                status: Status::Running,
            }],
            ..Library::default()
        },
        details_expanded: Some(second.id),
        ..Snapshot::default()
    };
    let (hits, labels) = render(snapshot);
    for action in [
        Action::Import,
        Action::ReadMore(first.id),
        Action::ReadMore(second.id),
        Action::Disable(second.id),
        Action::Reload(second.id),
        Action::ReloadAll,
        Action::Permission(second.id, 0, true),
        Action::Remove(second.id),
    ] {
        assert!(hits.contains(&command(action)), "{action:?}");
    }
    assert!(!hits.contains(&command(Action::Enable(second.id))));
    assert!(!hits.contains(&command(Action::Remove(first.id))));
    for label in [
        "Active mods (1)",
        "My mods (1)",
        "Running",
        "Ready to enable",
    ] {
        assert!(labels.iter().any(|found| found == label), "{label}");
    }
}

#[test]
fn each_active_card_exposes_its_own_reload_permissions_and_reorder_direction() {
    for (id, move_action, missing_action) in [
        (1, Action::MoveDown(1), Action::MoveUp(1)),
        (2, Action::MoveUp(2), Action::MoveDown(2)),
    ] {
        let snapshot = Snapshot {
            library: Library {
                installed: vec![installed(1, "first"), installed(2, "second")],
                active: vec![
                    ActiveMod {
                        id: 1,
                        status: Status::Running,
                    },
                    ActiveMod {
                        id: 2,
                        status: Status::ReloadFailed("invalid component".into()),
                    },
                ],
                ..Default::default()
            },
            details_expanded: Some(id),
            ..Default::default()
        };
        let (hits, labels) = render(snapshot);
        for action in [
            Action::Disable(id),
            Action::Reload(id),
            Action::Permission(id, 0, true),
            move_action,
        ] {
            assert!(hits.contains(&command(action)));
        }
        assert!(!hits.contains(&command(missing_action)));
        assert!(labels.iter().any(|label| label == "Active mods (2)"));
        if id == 2 {
            assert!(
                labels
                    .iter()
                    .any(|label| label == "Reload failed · previous instance running")
            );
            assert!(labels.iter().any(|label| label == "invalid component"));
        }
    }
}

#[test]
fn selecting_the_registered_mods_section_opens_its_controls() {
    let mut view = MenuView::new(true, "Player".into());
    view.settings_section = launcher::menu::settings_sections::SettingSection::Mods
        .definition()
        .index;
    let (_, hits, _) = paint(HashMap::new(), |canvas| {
        super::super::draw(canvas, &view, [1280.0, 720.0], &|_| None, None).unwrap();
    });
    assert!(
        hits.iter()
            .any(|(action, _)| *action == command(Action::Import))
    );
}

#[test]
fn declared_metadata_names_the_card_and_its_icon_replaces_the_placeholder() {
    let mut item = installed(3, "zoom-plus");
    item.metadata = ui::mod_metadata::ModMetadata {
        name: Some("Zoom Plus".into()),
        description: Some("Smooth zoom on any key.".into()),
        version: Some("v1.2.0".into()),
        authors: Some("Alex".into()),
        licenses: Some("MIT".into()),
        icon: Some(Arc::from([0u8].as_slice())),
    };
    let artwork = HashMap::from([(
        item.icon_key().unwrap(),
        IconRef {
            page: 9,
            uv: [0, 0, 32, 32],
            glint: false,
        },
    )]);
    let snapshot = Snapshot {
        library: Library {
            installed: vec![item.clone()],
            ..Library::default()
        },
        details_expanded: Some(item.id),
        ..Snapshot::default()
    };
    let (_, labels, nodes) = render_with(snapshot, &artwork);
    for label in [
        "Zoom Plus",
        "Ready to enable · v1.2.0",
        "Smooth zoom on any key.",
        "By Alex",
        "zoom-plus.wasm · 2.0 KB · MIT",
    ] {
        assert!(
            labels.iter().any(|found| found == label),
            "{label}: {labels:?}"
        );
    }
    assert!(!labels.iter().any(|label| label == "zoom-plus"));
    assert!(nodes.iter().any(|node| matches!(
        node.visual(),
        ui::UiVisual::Sprite {
            texture_page: 9,
            ..
        }
    )));
}

/// Keeps host capability indices stable when unrelated declarations are hidden.
#[test]
fn only_requested_permissions_are_visible_and_actionable() {
    let mut item = installed(7, "camera-mod");
    let camera = item.permissions[0].clone();
    item.permissions = vec![
        Permission {
            name: "terrain".into(),
            requested: Some(false),
            ..camera.clone()
        },
        camera.clone(),
        Permission {
            name: "unknown".into(),
            requested: None,
            ..camera
        },
    ];
    let (hits, labels) = render(Snapshot {
        library: Library {
            installed: vec![item],
            ..Default::default()
        },
        details_expanded: Some(7),
        ..Default::default()
    });
    assert!(hits.contains(&command(Action::Permission(7, 1, true))));
    assert!(!hits.contains(&command(Action::Permission(7, 0, true))));
    assert!(!hits.contains(&command(Action::Permission(7, 2, true))));
    assert!(labels.iter().any(|label| label.contains("camera")));
    assert!(
        !labels
            .iter()
            .any(|label| label.contains("terrain") || label.contains("unknown"))
    );
}

#[test]
fn toggle_all_targets_requested_permissions_and_respects_busy_state() {
    for (enabled, second_enabled, busy) in [
        (false, false, false),
        (true, true, false),
        (true, false, false),
        (false, false, true),
    ] {
        let mut item = installed(7, "camera-mod");
        item.permissions[0].enabled = enabled;
        item.permissions.push(Permission {
            name: "hud".into(),
            description: "Allows HUD output.".into(),
            enabled: second_enabled,
            requested: Some(true),
        });
        item.permissions.push(Permission {
            name: "terrain".into(),
            description: "Unrelated".into(),
            enabled: false,
            requested: Some(false),
        });
        let (hits, labels) = render(Snapshot {
            library: Library {
                installed: vec![item],
                busy,
                ..Default::default()
            },
            details_expanded: Some(7),
            ..Default::default()
        });
        assert!(labels.iter().any(|label| label == "Toggle All"));
        assert_eq!(
            hits.contains(&command(Action::RequestedPermissions(
                7,
                !enabled || !second_enabled
            ))),
            !busy
        );
    }
}

#[test]
fn no_requested_permissions_has_no_switches_or_bulk_action() {
    for requested in [None, Some(false)] {
        let mut item = installed(7, "camera-mod");
        item.permissions[0].requested = requested;
        let (hits, labels) = render(Snapshot {
            library: Library {
                installed: vec![item],
                ..Default::default()
            },
            details_expanded: Some(7),
            ..Default::default()
        });
        assert!(
            labels
                .iter()
                .any(|label| label == "No requested permissions.")
        );
        assert!(!labels.iter().any(|label| label == "Toggle All"));
        assert!(!hits.iter().any(|action| matches!(
            action,
            MenuAction::Mods(Action::Permission(..) | Action::RequestedPermissions(..))
        )));
    }
}

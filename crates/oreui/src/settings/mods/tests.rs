use {
    super::*,
    crate::review_tests::paint,
    launcher::menu::MenuView,
    launcher::mods::{ActiveMod, Library, Permission},
    std::{collections::HashMap, sync::Arc},
};

/// Supplies one card with a declared camera permission for interaction assertions.
fn installed(id: u64, name: &str) -> InstalledMod {
    InstalledMod {
        id,
        name: name.into(),
        file: format!("{name}.wasm"),
        bytes: 2048,
        permissions: vec![Permission {
            name: "camera".into(),
            description: "Allows camera output.".into(),
            enabled: false,
            requested: Some(true),
        }],
    }
}

/// Paints the page and exposes its visible text and identity-scoped hit actions.
fn render(snapshot: Snapshot) -> (Vec<MenuAction>, Vec<String>) {
    let mut view = MenuView::new(true, "Player".into());
    view.mods = Arc::new(snapshot);
    let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
        let mut content = Content {
            canvas,
            view: &view,
            span: [100.0, 900.0],
            column_width: 800.0,
            y: 100.0,
            translate: &|_| None,
            nested: false,
        };
        draw(&mut content).unwrap();
    });
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
    (hits.into_iter().map(|(action, _)| action).collect(), labels)
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

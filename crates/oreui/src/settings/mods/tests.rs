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
            requested_allowed: false,
            sensitive: true,
        }],
        metadata: Default::default(),
    }
}

#[test]
fn update_actions_target_only_the_mod_with_a_newer_release() {
    let mut snapshot = Snapshot::default();
    snapshot.library.installed = vec![installed(1, "zoom"), installed(2, "minimap")];
    snapshot
        .library
        .updates
        .push(launcher::mods::AvailableUpdate {
            id: 2,
            version: "v1.1.0".into(),
            release_url: "https://github.com/example/minimap/releases/tag/v1.1.0".into(),
        });
    let (hits, labels) = render(snapshot);
    assert!(hits.contains(&command(Action::OpenUpdate(2))));
    assert!(!hits.contains(&command(Action::OpenUpdate(1))));
    assert!(
        labels
            .iter()
            .any(|label| label == "Update available · v1.1.0")
    );
}

#[test]
fn downloadable_mod_update_button_installs_instead_of_opening_a_browser() {
    let mut snapshot = Snapshot::default();
    let mut item = installed(2, "zoom");
    item.metadata.update_url = Some(
        "https://github.com/example/zoom/releases/latest/download/zoom.wasm"
            .parse()
            .unwrap(),
    );
    snapshot.library.installed.push(item);
    snapshot
        .library
        .updates
        .push(launcher::mods::AvailableUpdate {
            id: 2,
            version: "1.1.0".into(),
            release_url: String::new(),
        });
    let (hits, labels) = render(snapshot.clone());
    assert!(hits.contains(&command(Action::Update(2))));
    assert!(!hits.contains(&command(Action::OpenUpdate(2))));
    assert!(labels.iter().any(|label| label == "Update"));
    snapshot.library.update_progress = vec![launcher::mods::UpdateProgress::stage(
        2,
        launcher::mods::UpdatePhase::Downloading,
    )];
    snapshot.library.busy = true;
    let (hits, labels) = render(snapshot);
    assert!(!hits.contains(&command(Action::Update(2))));
    assert!(labels.iter().any(|label| label == "Downloading…"));
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
    render_at(snapshot, artwork, 800.0, None, None)
}

/// Paints the same card at a chosen width with pointer or keyboard focus.
fn render_at(
    snapshot: Snapshot,
    artwork: &HashMap<String, IconRef>,
    width: f32,
    hovered: Option<MenuAction>,
    focused: Option<MenuAction>,
) -> (Vec<MenuAction>, Vec<String>, Vec<ui::UiNode>) {
    let mut view = MenuView::new(true, "Player".into());
    view.mods = Arc::new(snapshot);
    view.hovered = hovered;
    view.focused_action = focused;
    view.navigation_focus_visible = focused.is_some();
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
            span: [100.0, 100.0 + width],
            column_width: width,
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

#[test]
fn consent_offers_only_declared_requests_and_metadata_drives_warnings() {
    for sensitive in [false, true] {
        let mut item = installed(1, "zoom");
        item.permissions[0].sensitive = sensitive;
        let snapshot = Snapshot {
            library: Library {
                installed: vec![item],
                active: vec![ActiveMod {
                    id: 1,
                    status: Status::Running,
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let (hits, labels) = render(snapshot.clone());
        assert!(hits.contains(&command(Action::AllowRequested(1))));
        assert!(labels.iter().any(|label| label == "This mod requests:"));
        assert!(
            labels
                .iter()
                .any(|label| label == if sensitive { "⚠ camera" } else { "camera" })
        );
        let (_, labels) = render(Snapshot {
            customizing: Some(1),
            ..snapshot.clone()
        });
        assert!(
            labels
                .iter()
                .any(|label| label == if sensitive { "⚠ camera" } else { "camera" })
        );
        let mut undeclared = snapshot;
        undeclared.library.installed[0].permissions[0].requested = None;
        let (hits, labels) = render(undeclared.clone());
        assert!(
            labels
                .iter()
                .any(|label| label == "This mod doesn't declare its permissions")
        );
        assert!(!hits.contains(&command(Action::AllowRequested(1))));
        let (hits, _) = render(Snapshot {
            customizing: Some(1),
            ..undeclared
        });
        assert!(!hits.contains(&command(Action::AllowRequested(1))));
    }
}

#[test]
fn customization_orders_requests_first_and_details_follow_hover_or_focus() {
    let mut item = installed(1, "zoom");
    let mut extra = item.permissions[0].clone();
    extra.name = "HUD".into();
    extra.description = "Draws the HUD.".into();
    extra.requested = Some(false);
    extra.sensitive = false;
    item.permissions.insert(0, extra);
    let snapshot = Snapshot {
        library: Library {
            installed: vec![item],
            ..Default::default()
        },
        customizing: Some(1),
        ..Default::default()
    };
    let (hits, labels) = render(snapshot.clone());
    assert!(hits.contains(&command(Action::Permission(1, 1, true))));
    assert!(!hits.contains(&command(Action::Permission(1, 0, true))));
    assert!(hits.contains(&command(Action::RevokeAll(1))));
    assert!(hits.contains(&command(Action::ShowAllPermissions(1))));
    assert!(!labels.iter().any(|label| label == "Allows camera output."));
    let action = Some(command(Action::Permission(1, 1, true)));
    for (hovered, focused) in [(action, None), (None, action)] {
        let (_, labels, _) = render_at(snapshot.clone(), &HashMap::new(), 800.0, hovered, focused);
        assert!(labels.iter().any(|label| label == "Allows camera output."));
        assert!(!labels.iter().any(|label| label == "Draws the HUD."));
    }
    let all = Snapshot {
        show_all_permissions: true,
        ..snapshot
    };
    let (hits, _) = render(all.clone());
    let order: Vec<_> = hits
        .into_iter()
        .filter_map(|action| match action {
            MenuAction::Mods(Action::Permission(_, index, _)) => Some(index),
            _ => None,
        })
        .collect();
    assert_eq!(order, [1, 0]);
    for (width, columns) in [(800.0, 2), (300.0, 1)] {
        let (_, labels, nodes) = render_at(all.clone(), &HashMap::new(), width, None, None);
        let bounds: Vec<_> = labels
            .iter()
            .zip(
                nodes
                    .iter()
                    .filter(|node| matches!(node.visual(), ui::UiVisual::Text { .. })),
            )
            .filter(|(label, _)| label.as_str() == "HUD" || label.as_str() == "⚠ camera")
            .map(|(_, node)| node.bounds())
            .collect();
        assert_eq!(bounds.len(), 2);
        assert_eq!(bounds[0].min().y() == bounds[1].min().y(), columns == 2);
    }
}

/// Every native build exposes mod import without a feature availability branch.
#[test]
fn empty_library_offers_import() {
    let (hits, labels) = render(Snapshot::default());
    assert!(labels.iter().any(|label| label == "No mods installed"));
    assert!(hits.contains(&command(Action::Import)));
}

#[test]
fn collapsed_cards_show_requested_permission_summary_and_actions() {
    let snapshot = Snapshot {
        library: Library {
            installed: vec![installed(1, "zoom")],
            ..Default::default()
        },
        ..Default::default()
    };
    let (hits, labels) = render(snapshot);
    assert!(
        labels
            .iter()
            .any(|label| label == "Permissions: 0 of 1 requested allowed")
    );
    assert!(labels.iter().any(|label| label == "Customize"));
    assert!(hits.contains(&command(Action::Enable(1))));
    assert!(!hits.contains(&command(Action::Permission(1, 0, true))));
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
        customizing: Some(second.id),
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
    assert!(hits.contains(&command(Action::Remove(first.id))));
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
            customizing: Some(id),
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
    view.settings.section = launcher::menu::settings_sections::SettingSection::Mods
        .definition()
        .index;
    let (_, hits, _) = paint(HashMap::new(), |canvas| {
        super::super::draw(
            canvas,
            &view,
            [1280.0, 720.0],
            &|_| None,
            None,
            &mut Default::default(),
        )
        .unwrap();
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
        update_repository: None,
        update_url: None,
    };
    let artwork = HashMap::from([(
        item.icon_key().unwrap(),
        IconRef {
            color_mask: None,
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

#[test]
fn update_progress_shows_download_stats_and_distinguishes_installing_from_reloading() {
    use launcher::mods::{UpdatePhase, UpdateProgress};
    for phase in [
        UpdatePhase::Downloading,
        UpdatePhase::Verifying,
        UpdatePhase::Installing,
        UpdatePhase::Reloading,
    ] {
        let mut snapshot = Snapshot::default();
        let mut item = installed(2, "zoom");
        item.metadata.update_url = Some(
            "https://github.com/example/zoom/releases/latest/download/zoom.wasm"
                .parse()
                .unwrap(),
        );
        snapshot.library.installed.push(item);
        let mut progress = UpdateProgress::stage(2, phase);
        if phase == UpdatePhase::Downloading {
            progress.received_bytes = 512 * 1024;
            progress.total_bytes = 1024 * 1024;
            progress.bytes_per_sec = Some(256 * 1024);
            progress.eta_secs = Some(2);
        }
        snapshot.library.update_progress.push(progress);
        snapshot.library.busy = true;
        // Runtime reload progress remains visible after the newer-version offer has cleared.
        let (hits, labels) = render(snapshot);
        assert!(!hits.contains(&command(Action::Update(2))));
        assert!(labels.iter().any(|label| label.contains(phase.label())));
        assert_eq!(
            labels.iter().any(|label| label.contains("50%")),
            phase == UpdatePhase::Downloading
        );
        assert_eq!(
            labels
                .iter()
                .any(|label| label.contains("0.25MB/s") && label.contains("2s left")),
            phase == UpdatePhase::Downloading
        );
    }
}

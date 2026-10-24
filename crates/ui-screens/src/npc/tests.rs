use super::*;

/// Renders the installed screen so its collection bindings and geometry stay covered.
#[test]
fn dialogue_actions_have_labels_and_clickable_regions() {
    let Some(carrier) = ui_engine::test_support::carrier() else {
        return;
    };
    let catalog = json_ui::Catalog::from_files(
        carrier
            .ui_files()
            .iter()
            .map(|file| (&*file.path, &*file.bytes)),
    )
    .unwrap();
    let env = json_ui::LayoutEnv {
        text: &crate::test_support::FixedText,
        textures: &crate::test_support::NoTextures,
    };
    for count in [0, 1, 2, 3, 4, 7] {
        let npc = NpcDialogueForm {
            npc_unique_id: 42,
            npc_name: "Workshop guide".into(),
            scene_name: "workshop".into(),
            dialogue: "Choose a task.".into(),
            buttons: (0..count)
                .map(|index| protocol::NpcButton {
                    text: format!("Task {index}").into(),
                    is_url: index % 2 == 1,
                    action_index: index as u8,
                })
                .collect(),
            open: true,
        };
        let render = json_ui::render_screen(
            NPC_SCREEN,
            &catalog,
            &crate::menu_screens::retail_context(),
            &npc_data(&npc),
            [640.0, 360.0],
            &env,
            &ViewState::default(),
        )
        .unwrap();
        let texts: Vec<_> = render
            .nodes
            .iter()
            .filter_map(|node| {
                if let json_ui::Draw::Text { text, .. } = &node.draw {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(texts.contains(&"Workshop guide"));
        assert!(texts.contains(&"Choose a task."));
        let hits: Vec<_> = render
            .hits
            .iter()
            .filter(|hit| hit.pressed.as_deref() == Some("button.student_button"))
            .collect();
        assert_eq!(hits.len(), count, "{count} dialogue actions");
        for (index, hit) in hits.iter().enumerate() {
            assert!(hit.enabled && hit.rect.w > 0.0 && hit.rect.h > 0.0);
            assert_eq!(hit.collection_index, Some(index));
            assert!(hit.rect.x >= hit.clip.x && hit.rect.y >= hit.clip.y);
            assert!(hit.rect.x + hit.rect.w <= hit.clip.x + hit.clip.w);
            assert!(hit.rect.y + hit.rect.h <= hit.clip.y + hit.clip.h);
            for other in &hits[..index] {
                let overlap_width = (hit.rect.x + hit.rect.w).min(other.rect.x + other.rect.w)
                    - hit.rect.x.max(other.rect.x);
                let overlap_height = (hit.rect.y + hit.rect.h).min(other.rect.y + other.rect.h)
                    - hit.rect.y.max(other.rect.y);
                assert!(overlap_width <= 0.0 || overlap_height <= 0.0);
            }
            let label = format!("Task {index}");
            assert_eq!(
                texts.iter().filter(|text| **text == label.as_str()).count(),
                1,
                "{texts:?}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Complete module clients can publish independent widgets beyond the legacy sixteen.
    #[test]
    fn module_huds_accept_many_small_cards_and_preserve_legacy_shadows() {
        let cards: Vec<_> = (0..24)
            .map(|index| serde_json::json!({"id":format!("module_{index}"),"rows":[]}))
            .collect();
        let hud: Hud = serde_json::from_value(serde_json::json!({"cards":cards})).unwrap();
        assert!(hud.validate().is_ok());
        assert!(hud.cards.iter().all(|c| c.shadow));
        let card: Card = serde_json::from_value(
            serde_json::json!({"id":"inventory","rows":[],"row_layout":"grid","shadow":false}),
        )
        .unwrap();
        assert!(!card.shadow);
        assert_eq!(card.row_layout, RowLayout::Grid);
    }
    #[test]
    fn larger_card_catalog_and_actor_cosmetics_keep_explicit_bounds() {
        let mut hud:Hud=serde_json::from_value(serde_json::json!({"cards":(0..MAX_HUD_CARDS)
            .map(|index|serde_json::json!({"id":format!("card_{index}"),"rows":[]})).collect::<Vec<_>>(),
            "actor_style":{"hurt_color":[1.,0.,0.,0.3],"nametags":{"scale":1.,"shadow":true}}})).unwrap();
        assert!(hud.validate().is_ok());
        hud.actor_style
            .as_mut()
            .unwrap()
            .nametags
            .as_mut()
            .unwrap()
            .opacity = 1.01;
        assert!(hud.validate().is_err());
        hud.actor_style
            .as_mut()
            .unwrap()
            .nametags
            .as_mut()
            .unwrap()
            .opacity = 0.5;
        hud.actor_style.as_mut().unwrap().attackable_color = Some([1., 0., 0., 1.1]);
        assert!(hud.validate().is_err());
        hud.actor_style.as_mut().unwrap().attackable_color = Some([1., 0., 0., 0.3]);
        assert!(hud.validate().is_ok());
        hud.actor_style.as_mut().unwrap().hurt_color = Some([f32::NAN, 0., 0., 1.]);
        assert!(hud.validate().is_err());
        hud.actor_style = None;
        let mut excess = hud.cards[0].clone();
        excess.id = "excess".into();
        hud.cards.push(excess);
        assert!(hud.validate().is_err());
    }
    #[test]
    fn card_defaults_preserve_legacy_background_and_validate_new_presentation_fields() {
        let mut hud: Hud =
            serde_json::from_str(r#"{"cards":[{"id":"supplies","rows":[]}]}"#).unwrap();
        assert!(hud.cards[0].title.is_empty());
        assert_eq!(hud.cards[0].position, None);
        assert_eq!(hud.cards[0].background_opacity, 0.82);
        assert_eq!(hud.cards[0].row_layout, RowLayout::Standard);
        assert_eq!(hud.cards[0].width, DEFAULT_CARD_WIDTH);
        assert_eq!(hud.cards[0].row_height, DEFAULT_ROW_HEIGHT);
        assert_eq!(hud.cards[0].icon_size, DEFAULT_ICON_SIZE);
        assert_eq!(hud.cards[0].text_scale, DEFAULT_TEXT_SCALE);
        assert!(hud.validate().is_ok());
        for opacity in [0., 0.35, 1.] {
            hud.cards[0].background_opacity = opacity;
            assert!(hud.validate().is_ok());
        }
        for opacity in [f32::NAN, -0.01, 1.01] {
            hud.cards[0].background_opacity = opacity;
            assert!(hud.validate().is_err());
        }
        hud.cards[0].background_opacity = 0.82;
        for position in [[f32::NAN, 0.], [-0.01, 0.], [1.01, 0.], [0., f32::INFINITY]] {
            hud.cards[0].position = Some(position);
            assert!(hud.validate().is_err());
        }
        for position in [[0., 0.], [0.5, 0.5], [1., 1.]] {
            hud.cards[0].position = Some(position);
            assert!(hud.validate().is_ok());
        }
        hud.cards[0].editor_label = "Bad\nlabel".into();
        assert!(hud.validate().is_err());
    }
    #[test]
    fn row_layout_dimensions_are_bounded_and_icons_fit_their_rows() {
        for layout in ["standard", "stacked_text", "icon_right"] {
            let mut hud: Hud = serde_json::from_value(serde_json::json!({"cards":[{
                "id":"fixture","row_layout":layout,"width":64.,
                "row_height":32.,"icon_size":24.,"rows":[]
            }]}))
            .unwrap();
            assert!(hud.validate().is_ok());
            for dimensions in [
                [f32::NAN, 32., 24.],
                [47., 32., 24.],
                [513., 32., 24.],
                [64., f32::INFINITY, 24.],
                [64., 11., 4.],
                [64., 65., 24.],
                [64., 32., f32::NAN],
                [64., 32., 3.],
                [64., 64., 49.],
                [64., 20., 24.],
                [48., 48., 40.],
            ] {
                hud.cards[0].width = dimensions[0];
                hud.cards[0].row_height = dimensions[1];
                hud.cards[0].icon_size = dimensions[2];
                assert!(
                    hud.validate().is_err(),
                    "layout={layout}, dimensions={dimensions:?}"
                );
            }
        }
        assert!(
            serde_json::from_str::<Hud>(
                r#"{"cards":[{"id":"fixture","row_layout":"unknown","rows":[]}]}"#
            )
            .is_err()
        );
    }
    #[test]
    fn row_text_scale_is_optional_and_bounded_independently_of_geometry() {
        let mut hud: Hud =
            serde_json::from_str(r#"{"cards":[{"id":"fixture","rows":[]}]}"#).unwrap();
        assert_eq!(hud.cards[0].text_scale, 1.);
        for scale in [0.5, 1., 1.75, 2.] {
            hud.cards[0].text_scale = scale;
            assert!(hud.validate().is_ok());
        }
        for scale in [f32::NAN, f32::INFINITY, 0.49, 2.01] {
            hud.cards[0].text_scale = scale;
            assert!(hud.validate().is_err(), "text_scale={scale}");
        }
        let explicit: Hud =
            serde_json::from_str(r#"{"cards":[{"id":"fixture","text_scale":1.75,"rows":[]}]}"#)
                .unwrap();
        assert_eq!(explicit.cards[0].text_scale, 1.75);
        assert!(explicit.validate().is_ok());
    }
    #[test]
    fn editor_results_are_bounded_and_cancellation_carries_no_changes() {
        assert!(EditorResult::default().validate().is_ok());
        let saved = EditorResult {
            hidden_cards: Vec::new(),
            replace_pending_reset: false,
            saved: true,
            reset: false,
            placements: vec![Placement {
                id: "equipment".into(),
                anchor: Anchor::Center,
                offset: [0.; 2],
                scale: 1.,
                position: Some([0.5, 0.5]),
            }],
        };
        assert!(saved.validate().is_ok());
        let mut malformed_offset = saved.clone();
        malformed_offset.placements[0].offset = [f32::NAN, 0.];
        assert!(malformed_offset.validate().is_err());
        for scale in [f32::NAN, 0.24, 5.01] {
            let mut malformed = saved.clone();
            malformed.placements[0].scale = scale;
            assert!(malformed.validate().is_err());
        }
        assert!(
            EditorResult {
                saved: false,
                ..saved.clone()
            }
            .validate()
            .is_err()
        );
        let mut duplicate = saved.clone();
        duplicate.placements.push(duplicate.placements[0].clone());
        assert!(duplicate.validate().is_err());
        let mut excess = saved;
        excess.placements[0].position = Some([1.01, 0.]);
        assert!(excess.validate().is_err());
    }
    #[test]
    fn editor_autosave_is_opt_in_and_custom_actions_remain_bounded() {
        let mut hud: Hud = serde_json::from_str(r#"{"cards":[]}"#).unwrap();
        assert!(!hud.autosave && hud.surface.is_none());
        let done_last = format!("hud.done:{}", crate::mod_panel::MAX_PANEL_CONTROLS - 1);
        let done_excess = format!("hud.done:{}", crate::mod_panel::MAX_PANEL_CONTROLS);
        let card_last = format!("hud.card:{}", MAX_HUD_CARDS - 1);
        let card_excess = format!("hud.card:{}", MAX_HUD_CARDS);
        for (action, valid) in [
            ("hud.close", true),
            ("hud.done:0", true),
            (done_last.as_str(), true),
            (done_excess.as_str(), false),
            (card_last.as_str(), true),
            (card_excess.as_str(), false),
            ("mod.control:0", false),
            ("button.resume_game", false),
        ] {
            hud.surface = Some(crate::mod_panel::Surface {
                previews: Default::default(),
                collections: Default::default(),
                transitions: Default::default(),
                screen: "fixture.editor".into(),
                document: serde_json::json!({"namespace":"fixture","editor":{
                    "type":"button","button_mappings":[{"from_button_id":"button.menu_select",
                        "to_button_id":action,"mapping_type":"pressed"}]
                }})
                .to_string(),
                bindings: Default::default(),
            });
            assert_eq!(hud.validate().is_ok(), valid, "route {action}");
        }
    }
    #[test]
    fn crosshair_rejects_invisible_nonfinite_and_excessive_geometry() {
        for value in [f32::NAN, f32::INFINITY, 0., 17.] {
            assert!(
                Crosshair {
                    size: value,
                    ..Default::default()
                }
                .validate()
                .is_err()
            );
        }
        for alpha in [0., 0.0001, 0.49 / 255.] {
            assert!(
                Crosshair {
                    color: [1., 1., 1., alpha],
                    outline: 0.,
                    ..Default::default()
                }
                .validate()
                .is_err(),
                "foreground alpha must remain visible after 8-bit conversion"
            );
        }
        assert!(
            Crosshair {
                color: [1., 1., 1., 1. / 255.],
                ..Default::default()
            }
            .validate()
            .is_ok()
        );
        assert!(Crosshair::default().validate().is_ok());
    }
    #[test]
    fn hud_rejects_ambiguous_ids_formatting_and_invalid_progress() {
        let row = Row {
            localized: false,
            formatted: false,
            label: "Helmet".into(),
            value: "34 / 363".into(),
            item: Some("minecraft:diamond_helmet".into()),
            effect_id: None,
            metadata: 0,
            progress: Some(0.1),
            color: white(),
        };
        let card = Card {
            id: "armor".into(),
            title: "Armor".into(),
            anchor: Anchor::TopRight,
            offset: [-6., 6.],
            scale: 1.,
            rows: vec![row],
            ..Default::default()
        };
        let mut hud = Hud {
            cards: vec![card],
            ..Default::default()
        };
        assert!(hud.validate().is_ok());
        hud.cards.push(hud.cards[0].clone());
        assert!(hud.validate().is_err());
        hud.cards.pop();
        hud.cards[0].rows[0].progress = Some(1.1);
        assert!(hud.validate().is_err());
        hud.cards[0].rows[0].progress = None;
        hud.cards[0].title = "§khidden".into();
        assert!(hud.validate().is_err());
    }
}

#[cfg(test)]
mod cell_tests {
    use super::*;

    #[test]
    fn rectangular_cells_are_optional_and_bounded() {
        let mut hud: Hud = serde_json::from_str(r#"{"cards":[{"id":"input","width":56,"rows":[],"cells":[{"rect":[19,0,18,18],"label":"W"}]}]}"#).unwrap();
        assert!(hud.validate().is_ok());
        hud.cards[0].width = 32.;
        hud.cards[0].cells[0].rect = [11., 0., 10., 10.];
        assert!(hud.validate().is_ok());
        hud.cards[0].width = 56.;
        for rect in [
            [40., 0., 18., 18.],
            [0., 1010., 18., 18.],
            [0., 0., 0., 18.],
            [-1., 0., 18., 18.],
            [0., 0., f32::NAN, 18.],
        ] {
            hud.cards[0].cells[0].rect = rect;
            assert!(hud.validate().is_err(), "accepted {rect:?}");
        }
        hud.cards[0].cells[0].rect = [0., 0., 18., 18.];
        hud.cards[0].cells[0].background = [0., 0., 0., 2.];
        assert!(hud.validate().is_err());
        hud.cards[0].cells[0].background = [0.; 4];
        hud.cards[0].cells = vec![Cell::default(); MAX_CARD_CELLS];
        assert!(hud.validate().is_ok());
        hud.cards[0].cells.push(Cell::default());
        assert!(hud.validate().is_err());
    }
    #[test]
    fn rows_and_cells_retain_independent_card_and_total_limits() {
        let row: Row = serde_json::from_str(r#"{"label":"Observed","value":"1"}"#).unwrap();
        let row_card = Card {
            id: "rows".into(),
            rows: vec![row.clone(); MAX_CARD_ROWS],
            ..Default::default()
        };
        let mut single = Hud {
            cards: vec![row_card.clone()],
            ..Default::default()
        };
        assert!(single.validate().is_ok());
        single.cards[0].rows.push(row.clone());
        assert!(
            single.validate().is_err(),
            "the text-row limit stays unchanged"
        );
        single.cards[0].rows.pop();
        single.cards[0].cells.push(Cell::default());
        assert!(
            single.validate().is_err(),
            "rows and cells cannot share one card"
        );

        let mut hud = Hud::default();
        for index in 0..MAX_HUD_ROWS / MAX_CARD_ROWS {
            hud.cards.push(Card {
                id: format!("rows_{index}"),
                ..row_card.clone()
            });
        }
        for index in 0..MAX_HUD_CELLS / MAX_CARD_CELLS {
            hud.cards.push(Card {
                id: format!("cells_{index}"),
                cells: vec![Cell::default(); MAX_CARD_CELLS],
                ..Default::default()
            });
        }
        assert!(
            hud.validate().is_ok(),
            "full cell budgets must coexist with the existing full row budget"
        );
        hud.cards.push(Card {
            id: "extra_cell".into(),
            cells: vec![Cell::default()],
            ..Default::default()
        });
        assert!(
            hud.validate().is_err(),
            "the total cell limit remains bounded"
        );
        hud.cards.pop();
        hud.cards.push(Card {
            id: "extra_row".into(),
            rows: vec![row],
            ..Default::default()
        });
        assert!(
            hud.validate().is_err(),
            "the total row limit stays unchanged"
        );
    }
    #[test]
    fn formatted_cells_are_explicit_bounded_and_accept_native_style_codes() {
        let mut hud: Hud = serde_json::from_str(r#"{"cards":[{"id":"sidebar","rows":[],"cells":[{"label":"§aServer §lScore","formatted":true}]}]}"#).unwrap();
        assert!(hud.validate().is_ok());
        hud.cards[0].cells[0].formatted = false;
        assert!(hud.validate().is_err());
        hud.cards[0].cells[0].formatted = true;
        hud.cards[0].cells[0].label.push('\n');
        assert!(hud.validate().is_err());
    }
}
use super::*;

/// Native cards, factory resets and editor results accept the full published HUD scale range.
#[test]
fn uniform_card_scale_supports_quarter_to_five_without_widening_text_scale() {
    for scale in [super::MIN_CARD_SCALE, super::MAX_CARD_SCALE] {
        let hud = super::Hud {
            cards: vec![super::Card {
                id: "clock".into(),
                scale,
                reset_scale: Some(scale),
                ..Default::default()
            }],
            ..Default::default()
        };
        hud.validate().unwrap();
        super::EditorResult {
            hidden_cards: Vec::new(),
            replace_pending_reset: false,
            saved: true,
            reset: false,
            placements: vec![super::Placement {
                id: "clock".into(),
                anchor: super::Anchor::TopLeft,
                offset: [0.; 2],
                position: None,
                scale,
            }],
        }
        .validate()
        .unwrap();
    }
}

/// Studio's fixed action names use the same bounds as cards, controls and anchor choices.
#[test]
fn studio_surface_actions_accept_exact_names_and_reject_out_of_range_indices() {
    use crate::mod_panel::{MAX_PANEL_CONTROLS, Surface};
    let check = |action: &str| {
        let hud = super::Hud {
            surface: Some(Surface {
                screen: "studio.root".into(),
                document: serde_json::json!({"namespace":"studio","root":{"type":"screen","controls":[{"action":{
                    "type":"button", "size":[20,20], "button_mappings":[{"from_button_id":"button.menu_select","to_button_id":action,"mapping_type":"pressed"}]
                }}]}}).to_string(),
                bindings: Default::default(), transitions: Default::default(), collections: Default::default(), previews: Default::default(),
            }),
            ..Default::default()
        };
        hud.validate()
    };
    for action in [
        "hud.undo",
        "hud.redo",
        "hud.scale",
        "hud.scale_down",
        "hud.scale_up",
        "hud.save",
        "hud.cancel",
        "hud.close",
        "hud.reset",
        "hud.grid",
        "hud.snap",
    ] {
        check(action).unwrap();
    }
    for (prefix, bound) in [
        ("hud.visibility:", super::MAX_HUD_CARDS),
        ("hud.select:", super::MAX_HUD_CARDS),
        ("hud.card:", super::MAX_HUD_CARDS),
        ("hud.anchor:", 9),
        ("hud.control:", MAX_PANEL_CONTROLS),
        ("hud.done:", MAX_PANEL_CONTROLS),
    ] {
        for index in [0, bound - 1] {
            check(&format!("{prefix}{index}")).unwrap();
        }
        for suffix in [
            bound.to_string(),
            "-1".into(),
            "1.5".into(),
            "".into(),
            "99999999999999999999999999999".into(),
        ] {
            assert!(check(&format!("{prefix}{suffix}")).is_err());
        }
    }
    assert!(check("hud.toggle:0").is_err());
}

#[test]
fn amendment_visibility_and_inspector_fields_are_additive_and_validated() {
    let legacy: super::Hud = serde_json::from_value(serde_json::json!({
        "cards":[{"id":"fps","rows":[]}]
    }))
    .unwrap();
    assert!(legacy.cards[0].visible);
    assert!(super::Card::default().visible);
    assert_eq!(legacy.inspector_size, None);
    let mut hud: super::Hud = serde_json::from_value(serde_json::json!({
        "cards":[{"id":"fps","visible":false,"rows":[]}],
        "inspector_size":[180,240]
    }))
    .unwrap();
    hud.validate().unwrap();
    assert!(!hud.cards[0].visible);
    assert_eq!(
        serde_json::from_value::<super::Hud>(serde_json::to_value(&hud).unwrap()).unwrap(),
        hud
    );
    for value in [0., -1., f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for axis in 0..2 {
            let mut size = [180., 240.];
            size[axis] = value;
            hud.inspector_size = Some(size);
            assert!(hud.validate().unwrap_err().contains("inspector size"));
        }
    }
    for value in [
        serde_json::json!([12]),
        serde_json::json!([12, 24, 36]),
        serde_json::json!([12, "24"]),
    ] {
        assert!(
            serde_json::from_value::<super::Hud>(
                serde_json::json!({"cards":[],"inspector_size":value})
            )
            .is_err()
        );
    }
    assert!(
        serde_json::from_value::<super::Hud>(
            serde_json::json!({"cards":[{"id":"fps","rows":[],"visible":0}]})
        )
        .is_err()
    );
}

#[test]
fn studio_hidden_results_require_unique_saved_placement_ids() {
    let mut result = super::EditorResult {
        saved: true,
        placements: vec![super::Placement {
            id: "fps".into(),
            anchor: super::Anchor::TopLeft,
            offset: [0.; 2],
            scale: 1.,
            position: None,
        }],
        hidden_cards: vec!["fps".into()],
        ..Default::default()
    };
    result.validate().unwrap();
    result.hidden_cards.push("fps".into());
    assert!(result.validate().is_err());
    result.hidden_cards = vec!["unknown".into()];
    assert!(result.validate().is_err());
    result.hidden_cards = vec!["fps".into()];
    result.saved = false;
    assert!(result.validate().is_err());
}

#[test]
fn studio_native_placeholder_identity_survives_materialization_and_stays_out_of_json() {
    let mut card = super::Card {
        id: "effects_preview".into(),
        editor_label: "Effects".into(),
        source: Some(super::RowSource {
            kind: super::NativeRows::Effects {
                icons: true,
                stacked: false,
                warnings: false,
            },
            limit: 4,
            color: [1.; 4],
            brackets: false,
        }),
        ..Default::default()
    };
    card.ensure_editor_placeholder();
    assert_eq!(card.title, "Effects");
    card.source = None;
    card.title.clear();
    card.ensure_editor_placeholder();
    assert_eq!(card.title, "Effects");
    let mut json = serde_json::to_value(&card).unwrap();
    assert!(json.get("editor_native").is_none());
    json["editor_native"] = serde_json::json!(true);
    assert!(serde_json::from_value::<super::Card>(json).is_err());
    card.rows
        .push(serde_json::from_value(serde_json::json!({"label":"","value":"1"})).unwrap());
    card.title.clear();
    card.ensure_editor_placeholder();
    assert!(card.title.is_empty());
}

#[test]
fn studio_cancellation_cannot_replace_pending_reset_intent() {
    let result = super::EditorResult {
        replace_pending_reset: true,
        ..Default::default()
    };
    assert!(result.validate().is_err());
}

/// Background radii default to the original look and reject non-finite or out-of-range values.
#[test]
fn background_radii_default_to_the_original_look_and_are_bounded() {
    let mut hud: super::Hud = serde_json::from_value(serde_json::json!({
        "cards":[{"id":"fps","rows":[{"label":"","value":"60"}],"text_plate":{}}]
    }))
    .unwrap();
    assert_eq!(
        hud.cards[0].background_radius,
        super::DEFAULT_CARD_BACKGROUND_RADIUS
    );
    assert_eq!(hud.cards[0].text_plate.as_ref().unwrap().style.radius, 0.);
    hud.validate().unwrap();
    for radius in [0., 8., super::MAX_CARD_BACKGROUND_RADIUS] {
        hud.cards[0].background_radius = radius;
        hud.cards[0].text_plate.as_mut().unwrap().style.radius = radius;
        hud.validate().unwrap();
    }
    for radius in [
        -0.5,
        super::MAX_CARD_BACKGROUND_RADIUS + 0.5,
        f32::NAN,
        f32::INFINITY,
    ] {
        let mut card = hud.clone();
        card.cards[0].background_radius = radius;
        assert!(card.validate().is_err(), "card radius {radius}");
        let mut plate = hud.clone();
        plate.cards[0].text_plate.as_mut().unwrap().style.radius = radius;
        assert!(plate.validate().is_err(), "plate radius {radius}");
    }
}

/// Representative facts cannot be admitted as a real gameplay card.
#[test]
fn samples_require_preview_only_and_crosshairs_require_isolation() {
    use super::{Crosshair, Hud};
    let mut hud: Hud =
        serde_json::from_str(r#"{"cards":[{"id":"sample","sample":true,"rows":[]}] }"#).unwrap();
    assert!(hud.validate().is_err());
    hud.cards[0].preview_only = true;
    hud.validate().unwrap();
    hud.cards[0].crosshair = Some(Crosshair::default());
    hud.validate().unwrap();
    hud.cards[0].preview_only = false;
    assert!(hud.validate().is_err());
    hud.cards[0].preview_only = true;
    hud.cards[0]
        .rows
        .push(serde_json::from_value(serde_json::json!({"label":"","value":"1"})).unwrap());
    assert!(hud.validate().is_err());
}

/// Aliases reserve their complete native output and cannot collide or recursively expand.
#[test]
fn native_sample_aliases_reserve_rows_and_unique_card_identities() {
    use super::{Card, Hud, MAX_HUD_CARDS, MAX_HUD_ROWS};
    let mut hud:Hud=serde_json::from_str(r#"{"cards":[{"id":"inventory","preview_id":"inventory_sample","rows":[],"source":{"kind":{"kind":"inventory","hotbar":true},"limit":36}}]}"#).unwrap();
    hud.validate().unwrap();
    hud.cards[0].preview_id = Some("inventory".into());
    assert!(hud.validate().is_err());
    hud.cards[0].preview_id = Some("preview".into());
    hud.cards[0].sample = true;
    hud.cards[0].preview_only = true;
    assert!(hud.validate().is_err());
    hud.cards[0].sample = false;
    let template = hud.cards[0].clone();
    hud.cards = (0..MAX_HUD_CARDS / 2 + 1)
        .map(|index| Card {
            id: format!("card_{index}"),
            preview_id: Some(format!("sample_{index}")),
            ..template.clone()
        })
        .collect();
    assert!(hud.validate().is_err());
    hud.cards.truncate(MAX_HUD_CARDS / 2);
    assert!(
        hud.cards
            .iter()
            .map(|card| usize::from(card.source.as_ref().unwrap().limit) * 2)
            .sum::<usize>()
            > MAX_HUD_ROWS
    );
    assert!(hud.validate().is_err());
}

/// Trusted host previews can exceed cell-content bounds without widening JSON admission.
#[test]
fn host_preview_size_is_validated_and_not_serialized() {
    let mut hud = Hud {
        cards: vec![Card {
            id: "touch".into(),
            width: 1.,
            cells: vec![Cell {
                rect: [0., 0., 1., 1.],
                ..Default::default()
            }],
            editor_size: Some([2048., 1600.]),
            ..Default::default()
        }],
        ..Default::default()
    };
    assert!(hud.validate().is_ok());
    let encoded = serde_json::to_value(&hud).unwrap();
    assert!(encoded["cards"][0].get("editor_size").is_none());
    let decoded: Hud = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded.cards[0].editor_size, None);
    for size in [[f32::NAN, 1.], [1., f32::INFINITY], [0., 1.], [-1., 1.]] {
        hud.cards[0].editor_size = Some(size);
        assert!(hud.validate().is_err());
    }
}

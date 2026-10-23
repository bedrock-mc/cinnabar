//! The OreUI data-driven form: option lists that outgrow the window, and slider presses along
//! the thumb's travel.

use std::sync::Arc;

use oreui::data_driven_screen::{FormHit, FormItem};
use protocol::ddui::{DataValue, ScriptedScreen};
use protocol::test_support::ddui::{custom_form_property, show_events};

use super::*;
use crate::ui_runtime::data_driven::DataDrivenAction;

const SIZE: [u32; 2] = [1280, 720];

/// Shows the fixture form, its dropdown holding `options` choices.
fn shown(options: usize) -> (player_state::PlayerState, UiRuntime) {
    let mut property = custom_form_property();
    let items: std::collections::BTreeMap<Arc<str>, DataValue> = (0..options)
        .map(|index| {
            let item = DataValue::Map(
                [
                    (
                        Arc::from("label"),
                        DataValue::String(format!("Option {index}").into()),
                    ),
                    (Arc::from("value"), DataValue::Number(index as f64)),
                ]
                .into_iter()
                .collect(),
            );
            (Arc::from(index.to_string()), item)
        })
        .chain([(Arc::from("length"), DataValue::Int(options as i64))])
        .collect();
    let DataValue::Map(form) = &mut property else {
        panic!("the form property is an object");
    };
    let Some(DataValue::Map(layout)) = form.get_mut("layout") else {
        panic!("the form has a layout");
    };
    let Some(DataValue::Map(dropdown)) = layout.get_mut("5") else {
        panic!("layout entry 5 is the dropdown");
    };
    dropdown.insert("items".into(), DataValue::Map(items));

    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    for (sequence, event) in (1..).zip(show_events(ScriptedScreen::CustomForm, 4, 7, property)) {
        runtime
            .apply(
                &mut player,
                SequencedUiEvent {
                    session_id: 1,
                    fifo_sequence: sequence,
                    local_millis: sequence * 10,
                    server_tick: None,
                    event,
                },
            )
            .unwrap();
    }
    (player, runtime)
}

fn build(
    player: &player_state::PlayerState,
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
) {
    presentation
        .build(player, runtime, 1_000, SIZE, DpiScale::new(1.0).unwrap())
        .unwrap();
}

fn item(runtime: &UiRuntime, matches: impl Fn(&FormItem) -> bool) -> usize {
    runtime
        .data_driven_form()
        .unwrap()
        .items
        .iter()
        .find(|entry| matches(&entry.item))
        .unwrap()
        .key
}

fn bounds(presentation: &UiPresentationRuntime, hit: FormHit) -> Option<ui::UiRect> {
    presentation
        .data_driven_hits()
        .iter()
        .find_map(|(candidate, bounds)| (*candidate == hit).then_some(*bounds))
}

#[test]
fn a_long_option_list_stays_in_the_window_and_scrolls_to_its_last_option() {
    let (player, mut runtime) = shown(60);
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let dropdown = item(&runtime, |item| matches!(item, FormItem::Dropdown { .. }));
    runtime.data_driven_action(DataDrivenAction::Activate(dropdown));
    build(&player, &mut presentation, &runtime);

    let options: Vec<_> = presentation
        .data_driven_hits()
        .iter()
        .filter(|(hit, _)| matches!(hit, FormHit::Option(_)))
        .map(|(_, bounds)| *bounds)
        .collect();
    assert!(!options.is_empty());
    for option in &options {
        assert!(option.min().y() >= 0.0 && option.max().y() <= SIZE[1] as f32);
    }
    assert!(bounds(&presentation, FormHit::Option(59)).is_none());

    let inside = options[0];
    let point = UiPoint::new(
        (inside.min().x() + inside.max().x()) * 0.5,
        (inside.min().y() + inside.max().y()) * 0.5,
    )
    .unwrap();
    presentation.scroll_data_driven(Some(point), 100_000.0);
    build(&player, &mut presentation, &runtime);
    let last = bounds(&presentation, FormHit::Option(59)).expect("the last option scrolled in");
    assert!(last.min().y() >= 0.0 && last.max().y() <= SIZE[1] as f32);
}

#[test]
fn slider_presses_map_over_the_thumb_travel() {
    let (player, runtime) = shown(3);
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let slider = item(&runtime, |item| matches!(item, FormItem::Slider { .. }));
    build(&player, &mut presentation, &runtime);
    let track = bounds(&presentation, FormHit::Item(slider)).unwrap();
    let [left, right] = [track.min().x(), track.max().x()];
    let fraction = |x| presentation.data_driven_slider_fraction(slider, x).unwrap();
    // The thumb's centre stops half a thumb short of each end.
    assert_eq!(fraction(left + 1.0), 0.0);
    assert_eq!(fraction(right - 1.0), 1.0);
    assert!((fraction((left + right) * 0.5) - 0.5).abs() < 1e-4);
}

#[test]
fn a_non_finite_slider_value_still_paints_the_form() {
    use protocol::ddui::{DataDrivenUiEvent, DataPrimitive, DataStoreChange, DataStoreUpdate};
    let (mut player, mut runtime) = shown(3);
    runtime
        .apply(
            &mut player,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 10,
                local_millis: 100,
                server_tick: None,
                event: protocol::UiEvent::DataDrivenUi(DataDrivenUiEvent::Store(vec![
                    DataStoreChange::Update(DataStoreUpdate {
                        store: protocol::ddui::SCRIPT_STORE.into(),
                        property: ScriptedScreen::CustomForm.property(Some(7)).into(),
                        path: "layout[4].value".into(),
                        value: DataPrimitive::Number(f64::NAN),
                        property_update_count: 1,
                        path_update_count: 1,
                    }),
                ])),
            },
        )
        .unwrap();
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    build(&player, &mut presentation, &runtime);
    assert!(!presentation.data_driven_hits().is_empty());
}

#[test]
fn a_closed_form_leaves_no_targets_for_a_form_reusing_its_id() {
    use protocol::ddui::DataDrivenUiEvent;
    let (mut player, mut runtime) = shown(3);
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    build(&player, &mut presentation, &runtime);
    let shown = runtime
        .data_driven_ui()
        .active()
        .map(|active| active.instance);
    assert_eq!(presentation.data_driven_painted_form(), shown);
    runtime
        .apply(
            &mut player,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 10,
                local_millis: 100,
                server_tick: None,
                event: protocol::UiEvent::DataDrivenUi(DataDrivenUiEvent::Close {
                    form_id: Some(4),
                }),
            },
        )
        .unwrap();
    build(&player, &mut presentation, &runtime);
    assert_eq!(presentation.data_driven_painted_form(), None);
    assert!(presentation.data_driven_hits().is_empty());
}

#[test]
fn an_open_list_hides_while_its_dropdown_scrolls_out_of_view() {
    const SHORT: [u32; 2] = [1280, 140];
    let (player, mut runtime) = shown(3);
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let dropdown = item(&runtime, |item| matches!(item, FormItem::Dropdown { .. }));
    runtime.data_driven_action(DataDrivenAction::Activate(dropdown));
    let build_short = |presentation: &mut UiPresentationRuntime| {
        presentation
            .build(&player, &runtime, 1_000, SHORT, DpiScale::new(1.0).unwrap())
            .unwrap();
    };
    build_short(&mut presentation);
    // Scroll the column to its end, past the dropdown near its top.
    presentation.scroll_data_driven(None, 100_000.0);
    build_short(&mut presentation);
    assert!(bounds(&presentation, FormHit::Item(dropdown)).is_none());
    assert!(
        presentation
            .data_driven_hits()
            .iter()
            .all(|(hit, _)| !matches!(hit, FormHit::Option(_)))
    );
}

#[test]
fn a_replacement_reusing_the_form_id_is_a_new_instance() {
    use protocol::ddui::DataDrivenUiEvent;
    let (mut player, mut runtime) = shown(3);
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    build(&player, &mut presentation, &runtime);
    let first = runtime.data_driven_ui().active().unwrap().instance;
    assert_eq!(presentation.data_driven_painted_form(), Some(first));
    // The server closes form 4 and shows form 4 again before the next frame paints.
    let mut events = vec![protocol::UiEvent::DataDrivenUi(DataDrivenUiEvent::Close {
        form_id: Some(4),
    })];
    events.extend(show_events(
        ScriptedScreen::CustomForm,
        4,
        7,
        custom_form_property(),
    ));
    for (sequence, event) in (10..).zip(events) {
        runtime
            .apply(
                &mut player,
                SequencedUiEvent {
                    session_id: 1,
                    fifo_sequence: sequence,
                    local_millis: sequence * 10,
                    server_tick: None,
                    event,
                },
            )
            .unwrap();
    }
    let second = runtime.data_driven_ui().active().unwrap();
    assert_eq!(second.form_id, 4);
    assert_ne!(second.instance, first);
    // The last paint's targets belong to the replaced form.
    assert_ne!(
        presentation.data_driven_painted_form(),
        Some(second.instance)
    );
}

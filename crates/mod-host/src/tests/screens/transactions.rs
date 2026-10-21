//! Screen staging, rollback and callback isolation without building a guest package.

use super::{screen::Host as _, *};
use crate::{
    ModGrants, ModScreens,
    runtime::{Declared, Instance, commit_event},
};
use server_experience::policy::{MAX_COLLECTION_ROWS, MAX_COLLECTIONS, MAX_UI_VALUES};
use std::{collections::BTreeMap, sync::Arc};

/// Creates a screen-authorized host with enough declared edit controls to test its bounds.
fn state() -> State {
    State::new(
        ModGrants {
            screen: true,
            ..ModGrants::default()
        },
        String::new(),
        Declared {
            templates: ["ui/overlay.json".into(), "ui/view.json".into()].into(),
            actions: (0..=MAX_UI_VALUES)
                .map(|index| format!("edit{index}"))
                .collect(),
            ..Declared::default()
        },
    )
}

/// Reads the last accepted output, whether or not its callback has committed yet.
fn staged(state: &State) -> &ModScreens {
    state
        .pending_screens
        .as_ref()
        .map(ScreenTransaction::screens)
        .unwrap_or(&state.screens)
}

/// Compares every observable screen field, including edit and focus revisions.
fn same_screens(actual: &ModScreens, expected: &ModScreens) {
    let ModScreens {
        overlay,
        view,
        focus,
        data,
    } = actual;
    let host_screen::Modal {
        template,
        collections,
        values,
        texts,
        revision,
    } = data;
    assert_eq!(overlay, &expected.overlay);
    assert_eq!(view, &expected.view);
    assert_eq!(focus, &expected.focus);
    assert_eq!(template, &expected.data.template);
    assert_eq!(collections, &expected.data.collections);
    assert_eq!(values, &expected.data.values);
    assert_eq!(texts, &expected.data.texts);
    assert_eq!(revision, &expected.data.revision);
}

/// Checks that a denied import charges one call and leaves all accepted output untouched.
fn denied(
    state: &mut State,
    reason: &str,
    call: impl FnOnce(&mut State) -> Result<Result<(), String>>,
) {
    let before = staged(state).clone();
    let output = state.output;
    let calls = state.calls;
    let error = call(state).unwrap().unwrap_err();
    assert!(error.contains(reason), "{error}");
    same_screens(staged(state), &before);
    assert_eq!(state.output, output);
    assert_eq!(state.calls, calls + 1);
}

/// Makes one small, valid collection row with a distinguishable payload.
fn rows(value: i64) -> Vec<host_screen::Row> {
    vec![BTreeMap::from([(
        "#id".into(),
        host_screen::Value::Integer(value),
    )])]
}

#[test]
fn rejected_writes_preserve_earlier_successes_and_allow_later_writes() {
    let mut state = state();
    state
        .set_overlay(Some("ui/overlay.json".into()))
        .unwrap()
        .unwrap();
    state
        .set_value("#count".into(), ui::Value::Integer(1))
        .unwrap()
        .unwrap();
    denied(&mut state, "not a template", |state| {
        state.set_overlay(Some("ui/unknown.json".into()))
    });
    denied(&mut state, "not a template", |state| {
        state.open_view(Some("ui/unknown.json".into()))
    });
    denied(&mut state, "no container", |state| {
        state.open_view(Some("ui/view.json".into()))
    });
    denied(&mut state, "invalid binding", |state| {
        state.set_value("count".into(), ui::Value::Integer(9))
    });
    for value in [
        ui::Value::Number(f64::NAN),
        ui::Value::Numbers(vec![]),
        ui::Value::Text("invalid\0text".into()),
    ] {
        denied(&mut state, "invalid screen value", |state| {
            state.set_value("#count".into(), value)
        });
    }
    denied(&mut state, "invalid collection name", |state| {
        state.set_collection("bad name".into(), b"[]".to_vec())
    });
    denied(&mut state, "invalid rows", |state| {
        state.set_collection("items".into(), b"[".to_vec())
    });
    denied(&mut state, "too many collection rows", |state| {
        let rows = vec![host_screen::Row::new(); MAX_COLLECTION_ROWS + 1];
        state.set_collection("items".into(), serde_json::to_vec(&rows).unwrap())
    });
    denied(&mut state, "not a declared action", |state| {
        state.set_text("unknown".into(), "text".into())
    });
    denied(&mut state, "invalid edit box text", |state| {
        state.set_text("edit0".into(), "invalid\0text".into())
    });
    denied(&mut state, "not a declared action", |state| {
        state.focus_text("unknown".into())
    });
    state.grants.screen = false;
    denied(&mut state, "screen permission denied", |state| {
        state.set_value("#count".into(), ui::Value::Integer(9))
    });
    state.grants.screen = true;
    state
        .set_text("edit0".into(), "accepted".into())
        .unwrap()
        .unwrap();
    state.focus_text("edit0".into()).unwrap().unwrap();
    state
        .set_value("#count".into(), ui::Value::Integer(2))
        .unwrap()
        .unwrap();
    let screens = staged(&state);
    assert_eq!(
        screens.data.values["#count"],
        host_screen::Value::Integer(2)
    );
    assert_eq!(screens.data.texts["edit0"], (3, "accepted".into()));
    assert_eq!(screens.focus, Some((4, "edit0".into())));
    assert_eq!(screens.data.revision, 5);
    assert!(state.screens.data.values.is_empty());
    assert!(state.screens.overlay.is_none());
}

#[test]
fn aggregate_value_failure_rolls_back_only_the_rejected_entry() {
    let mut state = state();
    for index in 0..MAX_UI_VALUES - 1 {
        state
            .screens
            .data
            .set_value(format!("#v{index}"), host_screen::Value::Integer(0));
    }
    let before = state.screens.data.revision;
    state
        .set_value("#last".into(), ui::Value::Boolean(true))
        .unwrap()
        .unwrap();
    denied(&mut state, "value budget exceeded", |state| {
        state.set_value("#overflow".into(), ui::Value::Integer(1))
    });
    state
        .set_value("#v0".into(), ui::Value::Integer(2))
        .unwrap()
        .unwrap();
    let screens = staged(&state);
    assert_eq!(screens.data.values.len(), MAX_UI_VALUES);
    assert_eq!(screens.data.values["#last"], host_screen::Value::Bool(true));
    assert_eq!(screens.data.values["#v0"], host_screen::Value::Integer(2));
    assert!(!screens.data.values.contains_key("#overflow"));
    assert_eq!(screens.data.revision, before + 2);
}

#[test]
fn collection_replacement_at_the_aggregate_limit_survives_a_rejected_addition() {
    let mut state = state();
    for index in 0..MAX_COLLECTIONS - 1 {
        state
            .screens
            .data
            .set_collection(format!("items{index}"), rows(0));
    }
    let before = state.screens.data.revision;
    state
        .set_collection("last".into(), serde_json::to_vec(&rows(1)).unwrap())
        .unwrap()
        .unwrap();
    denied(&mut state, "collection budget exceeded", |state| {
        state.set_collection("overflow".into(), serde_json::to_vec(&rows(2)).unwrap())
    });
    state
        .set_collection("items0".into(), serde_json::to_vec(&rows(3)).unwrap())
        .unwrap()
        .unwrap();
    let screens = staged(&state);
    assert_eq!(screens.data.collections.len(), MAX_COLLECTIONS);
    assert_eq!(&*screens.data.collections["last"], rows(1));
    assert_eq!(&*screens.data.collections["items0"], rows(3));
    assert!(!screens.data.collections.contains_key("overflow"));
    assert_eq!(screens.data.revision, before + 2);
}

#[test]
fn invalid_collection_replacements_preserve_the_accepted_rows() {
    let mut state = state();
    state
        .set_collection("items".into(), serde_json::to_vec(&rows(1)).unwrap())
        .unwrap()
        .unwrap();
    for row in [
        host_screen::Row::from([("invalid".into(), host_screen::Value::Integer(0))]),
        host_screen::Row::from([(
            "#id".into(),
            host_screen::Value::Text("invalid\0text".into()),
        )]),
    ] {
        denied(&mut state, "invalid", |state| {
            state.set_collection("items".into(), serde_json::to_vec(&vec![row]).unwrap())
        });
    }
    state
        .set_collection("items".into(), serde_json::to_vec(&rows(2)).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(&*staged(&state).data.collections["items"], rows(2));
    assert_eq!(staged(&state).data.revision, 2);
}

#[test]
fn aggregate_text_failure_preserves_edit_revisions_and_focus_order() {
    let mut state = state();
    for index in 0..MAX_UI_VALUES - 1 {
        state
            .screens
            .data
            .set_text(format!("edit{index}"), "old".into());
    }
    let before = state.screens.data.revision;
    state
        .set_text(format!("edit{}", MAX_UI_VALUES - 1), "last".into())
        .unwrap()
        .unwrap();
    state.focus_text("edit0".into()).unwrap().unwrap();
    denied(&mut state, "edit box budget exceeded", |state| {
        state.set_text(format!("edit{MAX_UI_VALUES}"), "overflow".into())
    });
    state
        .set_text("edit0".into(), "new".into())
        .unwrap()
        .unwrap();
    let screens = staged(&state);
    assert_eq!(screens.data.texts.len(), MAX_UI_VALUES);
    assert_eq!(screens.data.texts["edit0"], (before + 3, "new".into()));
    assert_eq!(screens.focus, Some((before + 2, "edit0".into())));
    assert_eq!(screens.data.revision, before + 3);
}

#[test]
fn denied_only_callbacks_reuse_scratch_without_publishing_output() {
    let mut state = state();
    state.grants.screen = false;
    denied(&mut state, "permission denied", |state| {
        state.set_overlay(None)
    });
    state.grants.screen = true;
    denied(&mut state, "not a template", |state| {
        state.set_overlay(Some("ui/unknown.json".into()))
    });
    for index in 0..MAX_UI_VALUES {
        state
            .screens
            .data
            .set_value(format!("#v{index}"), host_screen::Value::Integer(0));
    }
    denied(&mut state, "value budget exceeded", |state| {
        state.set_value("#overflow".into(), ui::Value::Integer(1))
    });
    let key = staged(&state)
        .data
        .values
        .first_key_value()
        .unwrap()
        .0
        .as_ptr();
    denied(&mut state, "value budget exceeded", |state| {
        state.set_value("#overflow".into(), ui::Value::Integer(2))
    });
    assert_eq!(
        staged(&state)
            .data
            .values
            .first_key_value()
            .unwrap()
            .0
            .as_ptr(),
        key
    );
    assert!(state.pending_screens.unwrap().commit().is_none());
    assert_eq!(state.output, 0);
}

#[test]
fn accepted_output_may_reach_the_exact_byte_limit() {
    let mut state = state();
    let mut json = b"[{}]".to_vec();
    json.resize(MAX_HOST_OUTPUT, b' ');
    state.set_collection("items".into(), json).unwrap().unwrap();
    assert_eq!(state.output, MAX_HOST_OUTPUT);
    state
        .set_overlay(Some("ui/overlay.json".into()))
        .unwrap()
        .unwrap();
    denied(&mut state, "host output budget exceeded", |state| {
        state.set_value("#value".into(), ui::Value::Boolean(true))
    });
    assert_eq!(staged(&state).data.revision, 2);
}

#[test]
fn denied_calls_and_reads_share_the_exact_host_call_limit() {
    let mut state = state();
    denied(&mut state, "not a template", |state| {
        state.set_overlay(Some("undeclared".into()))
    });
    for _ in 1..MAX_HOST_CALLS - 1 {
        assert!(state.layout().unwrap().is_none());
    }
    state
        .set_overlay(Some("ui/overlay.json".into()))
        .unwrap()
        .unwrap();
    assert_eq!(state.calls, MAX_HOST_CALLS);
    let before = staged(&state).clone();
    let error = state.set_overlay(None).unwrap_err();
    assert!(error.to_string().contains("host-call limit exceeded"));
    same_screens(staged(&state), &before);
}

#[test]
fn counted_json_bytes_match_the_previous_buffer_encoding() {
    for value in [
        host_screen::Value::Bool(false),
        host_screen::Value::Integer(i64::MIN),
        host_screen::Value::Number(-0.0),
        host_screen::Value::Number(f64::INFINITY),
        host_screen::Value::Text("quotes \" slash \\ newline\n العربية".into()),
        host_screen::Value::Numbers(vec![f64::MIN, f64::MAX, f64::NAN]),
    ] {
        assert_eq!(
            screen_write::value_bytes(&value),
            serde_json::to_vec(&value).map_or(0, |json| json.len())
        );
    }
    let mut state = state();
    let name = "#text";
    let text = "quotes \" slash \\ newline\n العربية";
    state
        .set_value(name.into(), ui::Value::Text(text.into()))
        .unwrap()
        .unwrap();
    assert_eq!(
        state.output,
        name.len()
            + serde_json::to_vec(&host_screen::Value::Text(text.into()))
                .unwrap()
                .len()
    );
}

/// Builds retained output large enough to expose a full-map copy on every scalar write.
fn retained_screens() -> ModScreens {
    let mut screens = ModScreens::default();
    let rows = (0..MAX_COLLECTION_ROWS / 2)
        .flat_map(|index| rows(index as i64))
        .collect();
    screens.data.set_collection("retained".into(), rows);
    screens.data.set_value(
        "#retained".into(),
        host_screen::Value::Text("retained value".into()),
    );
    screens
}

#[test]
fn repeated_scalar_writes_reuse_untouched_map_entries() {
    let mut state = state();
    state.screens = retained_screens();
    state
        .set_value("#counter".into(), ui::Value::Integer(0))
        .unwrap()
        .unwrap();
    let key = staged(&state)
        .data
        .collections
        .first_key_value()
        .unwrap()
        .0
        .as_ptr();
    let host_screen::Value::Text(text) = &staged(&state).data.values["#retained"] else {
        panic!("retained text missing");
    };
    let text = text.as_ptr();
    for index in 1..32 {
        state
            .set_value("#counter".into(), ui::Value::Integer(index))
            .unwrap()
            .unwrap();
        assert_eq!(
            staged(&state)
                .data
                .collections
                .first_key_value()
                .unwrap()
                .0
                .as_ptr(),
            key
        );
        let host_screen::Value::Text(value) = &staged(&state).data.values["#retained"] else {
            panic!("retained text missing");
        };
        assert_eq!(value.as_ptr(), text);
    }
    assert!(Arc::ptr_eq(
        &staged(&state).data.collections["retained"],
        &state.screens.data.collections["retained"]
    ));
}

/// Compares an accepted import to the former clone, mutate, check and replace behavior.
fn equivalent(
    state: &mut State,
    expected: &mut ModScreens,
    call: impl FnOnce(&mut State) -> Result<Result<(), String>>,
    write: impl FnOnce(&mut ModScreens),
) {
    let mut next = expected.clone();
    write(&mut next);
    next.data.check().unwrap();
    call(state).unwrap().unwrap();
    *expected = next;
    same_screens(staged(state), expected);
}

#[test]
fn staged_output_matches_clone_per_write_with_a_large_retained_collection() {
    let mut state = state();
    state.screens = retained_screens();
    state.layout = Some(host_screen::ScreenLayout {
        screen: "inventory".into(),
        size: host_screen::GuiSize {
            width: 480.0,
            height: 270.0,
            scale: 2.0,
        },
        gui: Rect::default(),
        exclusions: vec![],
        view: None,
    });
    let committed = state.screens.clone();
    let mut expected = committed.clone();
    equivalent(
        &mut state,
        &mut expected,
        |state| state.set_overlay(Some("ui/overlay.json".into())),
        |screens| {
            screens.overlay = Some("ui/overlay.json".into());
            screens.data.revision += 1;
        },
    );
    equivalent(
        &mut state,
        &mut expected,
        |state| state.open_view(Some("ui/view.json".into())),
        |screens| {
            screens.view = Some("ui/view.json".into());
            screens.data.revision += 1;
        },
    );
    for index in 0..32 {
        equivalent(
            &mut state,
            &mut expected,
            |state| state.set_value("#counter".into(), ui::Value::Integer(index)),
            |screens| {
                screens
                    .data
                    .set_value("#counter".into(), host_screen::Value::Integer(index));
            },
        );
    }
    equivalent(
        &mut state,
        &mut expected,
        |state| state.set_text("edit0".into(), "search".into()),
        |screens| {
            screens.data.set_text("edit0".into(), "search".into());
        },
    );
    equivalent(
        &mut state,
        &mut expected,
        |state| state.focus_text("edit0".into()),
        |screens| {
            screens.data.revision += 1;
            screens.focus = Some((screens.data.revision, "edit0".into()));
        },
    );
    same_screens(&state.screens, &committed);
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let mut store = wasmtime::Store::new(&engine, state);
    commit_event(&mut store);
    same_screens(&store.data().screens, &expected);
    assert!(store.data().pending_screens.is_none());
}

#[test]
fn callbacks_commit_once_reset_budgets_and_discard_trapped_screens() {
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let initial = state();
    let mut instance = Instance::new(
        &engine,
        br#"(component
            (core module $m (func (export "init")) (func (export "frame") unreachable))
            (core instance $i (instantiate $m))
            (func (export "init") (canon lift (core func $i "init")))
            (func (export "frame") (canon lift (core func $i "frame"))))"#,
        initial.grants,
        String::new(),
        initial.declared,
        None,
    )
    .unwrap();
    instance
        .run(crate::FRAME_FUEL, commit_event, |_, store| {
            store
                .data_mut()
                .set_value("#first".into(), ui::Value::Integer(1))?
                .unwrap();
            assert!(store.data().screens.data.values.is_empty());
            Ok(())
        })
        .unwrap();
    assert_eq!(
        instance.store.data().screens.data.values["#first"],
        host_screen::Value::Integer(1)
    );
    instance
        .run(crate::FRAME_FUEL, commit_event, |_, store| {
            let state = store.data_mut();
            assert_eq!((state.calls, state.output), (0, 0));
            assert!(state.pending_screens.is_none());
            state
                .set_value("#second".into(), ui::Value::Integer(2))?
                .unwrap();
            assert!(staged(state).data.values.contains_key("#first"));
            Ok(())
        })
        .unwrap();
    let before = instance.store.data().screens.clone();
    let key = instance
        .store
        .data()
        .screens
        .data
        .values
        .first_key_value()
        .unwrap()
        .0
        .as_ptr();
    instance
        .run(crate::FRAME_FUEL, commit_event, |_, store| {
            assert!(store.data().pending_screens.is_none());
            Ok(())
        })
        .unwrap();
    same_screens(&instance.store.data().screens, &before);
    assert_eq!(
        instance
            .store
            .data()
            .screens
            .data
            .values
            .first_key_value()
            .unwrap()
            .0
            .as_ptr(),
        key
    );
    assert!(
        instance
            .run(crate::FRAME_FUEL, commit_event, |exports, store| {
                store
                    .data_mut()
                    .set_value("#trapped".into(), ui::Value::Boolean(true))?
                    .unwrap();
                exports.frame(store)
            })
            .is_err()
    );
    assert!(!instance.is_active());
    assert!(instance.store.data().pending_screens.is_none());
    same_screens(&instance.store.data().screens, &ModScreens::default());
}

#[test]
fn held_modifiers_report_what_the_host_last_set() {
    let mut state = state();
    assert_eq!(state.held_modifiers().unwrap(), Modifiers::empty());
    state.events.modifiers = crate::KeyModifiers {
        shift: true,
        ctrl: true,
        alt: false,
    };
    assert_eq!(
        state.held_modifiers().unwrap(),
        Modifiers::SHIFT | Modifiers::CTRL
    );
}

#[test]
fn elapsed_seconds_count_up_from_the_instance() {
    let mut state = state();
    let first = state.elapsed_seconds().unwrap();
    let second = state.elapsed_seconds().unwrap();
    assert!(first >= 0.0 && second >= first);
}

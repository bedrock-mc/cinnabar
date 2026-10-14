use std::collections::BTreeSet;

use super::*;

/// Loads sorted names by category; names in another category cannot validate a declaration.
fn vocabulary() -> BTreeSet<(&'static str, &'static str)> {
    let text = include_str!("../../data/vanilla-ui-vocabulary-1.26.50.txt");
    assert!(text.lines().zip(text.lines().skip(1)).all(|(a, b)| a < b));
    text.lines()
        .map(|row| {
            let (kind, name) = row.split_once('\t').expect("categorized UI vocabulary row");
            assert!(!name.is_empty() && !name.contains('\t'));
            (kind, name)
        })
        .collect()
}

/// Reports every unknown declaration, keeping diagnostics tied to its category.
fn unknown<'a>(
    vocabulary: &BTreeSet<(&str, &str)>,
    names: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<(&'a str, &'a str)> {
    names
        .into_iter()
        .filter(|name| !vocabulary.contains(name))
        .collect()
}

#[test]
fn every_contract_name_exists_in_the_pinned_vocabulary() {
    let vocabulary = vocabulary();
    let missing = unknown(&vocabulary, contract_names());
    assert!(
        missing.is_empty(),
        "unknown JSON-UI contract names: {missing:?}"
    );
    let mut unique = BTreeSet::new();
    for name in contract_names() {
        assert!(
            unique.insert(name),
            "duplicate contract declaration: {name:?}"
        );
    }
}

#[test]
fn stale_names_and_names_from_the_wrong_category_are_rejected() {
    let vocabulary = vocabulary();
    let names = [
        ("binding", "#is_appearance_visible"),
        ("binding", "#form_button_length"),
        ("binding", "#custom_dropdown"),
        ("binding", "#is_using_keyboard"),
        ("button", "button.menu_friends"),
        ("button", "button.close_emote_popup"),
        ("binding", Reference::HudHudScreen.as_str()),
        ("renderer", Button::MenuCancel.as_str()),
    ];
    assert_eq!(unknown(&vocabulary, names), names);
}

#[test]
fn engine_screen_policy_uses_only_declared_references() {
    let references: BTreeSet<_> = Reference::ALL.iter().map(|name| name.as_str()).collect();
    for screen in crate::ENGINE_SCREENS {
        assert!(
            references.contains(screen),
            "screen missing from contract: {screen}"
        );
        assert!(crate::is_engine_screen(screen));
    }
}

#[test]
fn context_keys_remove_only_the_variable_prefix() {
    for variable in Variable::ALL {
        assert_eq!(variable.as_str().strip_prefix('$'), Some(variable.key()));
    }
}

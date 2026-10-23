use super::*;

#[test]
fn shared_fixture_has_unique_public_identities_and_valid_policy() {
    let catalog = catalog();
    assert_eq!(catalog.schema_version, 1);
    assert!(catalog.demo);
    for role in catalog.roles.keys() {
        let account = &catalog.accounts[&format!("{role}-demo")];
        assert_eq!(&account.role, role);
        assert!(!account.name.trim().is_empty());
        assert!(!account.gamertag.trim().is_empty());
    }
    let mut ids = BTreeSet::new();
    let mut slugs = BTreeSet::new();
    for item in &catalog.listings {
        assert!(ids.insert(item.id));
        assert!(slugs.insert(&item.slug));
        assert!(!item.permissions.is_empty());
        let account = &catalog.accounts[&item.owner];
        assert_eq!(item.creator, account.name);
        assert!(catalog.roles.contains_key(&account.role));
    }
    for rule in &catalog.review_transitions {
        assert!(
            catalog
                .roles
                .values()
                .any(|values| values.contains(&rule.permission))
        );
        assert!(matches!(rule.owner.as_str(), "self" | "other"));
    }
}

#[test]
fn only_published_listings_can_be_opened_or_saved() {
    let catalog = catalog();
    let mut state = State::default();
    state.apply(Action::Browse);
    for item in &catalog.listings {
        state.apply(Action::Details(item.id));
        state.apply(Action::Save(item.id));
        if item.status == "published" {
            assert_eq!(state.details, Some(item.id));
            assert!(state.saved.contains(&item.id));
        } else {
            assert_ne!(state.details, Some(item.id));
            assert!(!state.saved.contains(&item.id));
        }
    }
    state.apply(Action::Details(u64::MAX));
    state.apply(Action::Save(u64::MAX));
    assert!(!state.saved.contains(&u64::MAX));
    assert!(
        state
            .listings()
            .iter()
            .all(|item| item.status == "published")
    );
}

#[test]
fn library_and_categories_filter_without_installing() {
    let mut state = State::default();
    state.apply(Action::Library);
    assert!(state.listings().is_empty());
    let item = catalog()
        .listings
        .iter()
        .find(|item| item.status == "published")
        .unwrap();
    state.apply(Action::Save(item.id));
    assert_eq!(state.listings().len(), 1);
    state.apply(Action::Save(item.id));
    assert!(state.listings().is_empty());
    state.apply(Action::Browse);
    for (index, category) in catalog().categories().iter().enumerate() {
        state.apply(Action::Category(Some(index)));
        assert!(
            state
                .listings()
                .iter()
                .all(|item| item.category == *category)
        );
    }
    state.apply(Action::Close);
    state.apply(Action::Save(item.id));
    assert!(!state.open);
    assert!(state.saved.is_empty());
}

#[test]
fn review_requires_an_authorized_independent_actor_and_reason() {
    let catalog = catalog();
    let item = catalog
        .listings
        .iter()
        .find(|item| item.status == "pending")
        .unwrap();
    for role in ["player", "creator", "unknown"] {
        assert_eq!(
            catalog.next_status(role, "reviewer", item, "approve", "Checked permissions"),
            None
        );
    }
    assert_eq!(
        catalog.next_status("moderator", &item.owner, item, "approve", "Own mod"),
        None
    );
    assert_eq!(
        catalog.next_status("moderator", "reviewer", item, "approve", "  "),
        None
    );
    assert_eq!(
        catalog.next_status("moderator", "", item, "approve", "Checked"),
        None
    );
    assert_eq!(
        catalog.next_status(
            "moderator",
            "reviewer",
            item,
            "approve",
            "Checked permissions"
        ),
        Some("published")
    );
    assert_eq!(
        catalog.next_status(
            "admin",
            "reviewer",
            item,
            "request_changes",
            "Explain position access"
        ),
        Some("changes_requested")
    );
    assert_eq!(
        catalog.next_status("admin", "reviewer", item, "suspend", "Not published"),
        None
    );
    assert!(!catalog.permits("moderator", "staff.manage"));
    assert!(catalog.permits("admin", "staff.manage"));
}

#[test]
fn navigation_never_becomes_a_mod_worker_command_or_changes_grants() {
    let mut snapshot = crate::mods::Snapshot::default();
    let library = snapshot.library.clone();
    for action in [Action::Browse, Action::Library, Action::Close] {
        assert_eq!(
            snapshot.apply(crate::mods::Action::Marketplace(action)),
            None
        );
        assert_eq!(snapshot.library, library);
    }
}

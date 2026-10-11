use {super::*, launcher::global_resources::Snapshot};

fn pack() -> InstalledPack {
    InstalledPack {
        id: "11111111-1111-4111-8111-111111111111".parse().unwrap(),
        name: "Test".into(),
        description: String::new(),
        version: [1, 0, 0],
        min_engine_version: None,
        revision: 0,
        subpacks: Vec::new(),
    }
}

#[test]
fn activation_and_deactivation_crossfade_the_card_without_retaining_old_actions() {
    let pack = pack();
    let mut snapshot = Snapshot {
        available: vec![pack.clone()],
        ..Default::default()
    };
    let mut motion = Resources::default();
    motion.sync(&snapshot, 0.0, true);
    snapshot.available.clear();
    snapshot.active.push(pack.clone());
    motion.sync(&snapshot, 1.0, true);
    let old = motion
        .cards
        .iter()
        .find(|card| card.group == Group::Available)
        .unwrap();
    assert!(!old.present && old.visibility == 1.0);
    assert_eq!(
        motion
            .cards
            .iter()
            .find(|card| card.group == Group::Active)
            .unwrap()
            .visibility,
        0.0
    );
    motion.sync(&snapshot, 1.06, true);
    assert!(
        motion
            .cards
            .iter()
            .all(|card| card.visibility > 0.0 && card.visibility < 1.0)
    );
    motion.sync(&snapshot, 1.3, true);
    assert_eq!(motion.cards.len(), 1);
    assert!(motion.cards[0].present && motion.cards[0].group == Group::Active);
    snapshot.active.clear();
    snapshot.available.push(pack);
    motion.sync(&snapshot, 2.0, true);
    motion.sync(&snapshot, 2.06, true);
    assert!(
        motion
            .cards
            .iter()
            .all(|card| card.visibility > 0.0 && card.visibility < 1.0)
    );
    motion.sync(&snapshot, 2.1, false);
    assert_eq!(motion.cards.len(), 1);
    assert_eq!(motion.cards[0].visibility, 1.0);
    assert!(motion.cards[0].present && motion.cards[0].group == Group::Available);
    motion.end_frame();
    motion.end_frame();
    assert!(motion.cards.is_empty());
}

#[test]
fn disclosure_reversal_starts_from_the_current_height() {
    let mut snapshot = Snapshot::default();
    let mut motion = Resources::default();
    motion.sync(&snapshot, 0.0, true);
    snapshot.active_expanded = false;
    motion.sync(&snapshot, 1.0, true);
    motion.sync(&snapshot, 1.04, true);
    let current = motion.expanded[0];
    assert!(current > 0.0 && current < 1.0);
    snapshot.active_expanded = true;
    motion.sync(&snapshot, 1.04, true);
    assert_eq!(motion.expanded[0], current);
    motion.sync(&snapshot, 1.08, true);
    assert!(motion.expanded[0] > current);
}

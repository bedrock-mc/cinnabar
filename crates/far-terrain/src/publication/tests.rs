use super::*;

/// A three-section column with independently budgeted physical publications.
struct Column {
    target: TerrainTarget,
    state: TerrainPublication,
    live: HashMap<SubChunkKey, u64>,
    keys: [SubChunkKey; 3],
    far_ready: bool,
}

impl Column {
    /// Starts with a complete near drawer and a resident far replacement underneath it.
    fn new() -> Self {
        let column = ChunkKey::new(0, 5, 0);
        let keys = std::array::from_fn(|y| SubChunkKey::from_chunk(column, y as i32));
        let live: HashMap<_, _> = keys.iter().map(|&key| (key, 1)).collect();
        let target = TerrainTarget {
            centre: ChunkKey::new(0, 0, 0),
            horizon: 32,
            near: std::sync::Arc::new(HashMap::from([(column, live.clone())])),
            retained: std::sync::Arc::new(keys.into_iter().collect()),
            empty_far: std::sync::Arc::new(HashSet::new()),
        };
        let mut value = Self {
            target,
            state: Default::default(),
            live,
            keys,
            far_ready: true,
        };
        value.frame();
        value
    }

    /// Publishes the frame and checks a real near allocation or the far drawer at every height.
    fn frame(&mut self) {
        let far_ready = self.far_ready;
        self.state.publish(&self.target, &self.live, |_| far_ready);
        let column = self.keys[0].chunk();
        let masked = self.state.near_columns().any(|key| key == column);
        for key in self.keys {
            let near_draw = self.state.draws_near(column) && self.live.contains_key(&key);
            let far_draw = self.far_ready && !masked;
            assert_eq!(
                usize::from(near_draw) + usize::from(far_draw),
                1,
                "one drawer for {key:?}; main acknowledgements do not participate"
            );
        }
    }

    /// Models a bounded actual render removal only after the preceding draw publication.
    fn remove_one(&mut self, key: SubChunkKey, retirement_room: bool) -> bool {
        if !retirement_room
            || self
                .state
                .near_columns()
                .any(|column| column == key.chunk())
        {
            return false;
        }
        self.live.remove(&key);
        self.state.withdraw(key, 2);
        true
    }

    /// Publishes a new main-world target independently of physical work or acknowledgement.
    fn request_near(&mut self, generation: u64) {
        Arc::make_mut(&mut self.target.near).insert(
            self.keys[0].chunk(),
            self.keys.iter().map(|&key| (key, generation)).collect(),
        );
    }
}

#[test]
fn outgoing_draw_switch_precedes_budget_split_removals_and_late_main_acknowledgements() {
    let mut column = Column::new();
    Arc::make_mut(&mut column.target.near).clear();
    // Main has requested withdrawal but render publication has not switched drawers yet.
    assert!(!column.remove_one(column.keys[0], true));
    column.frame();
    for key in column.keys {
        assert!(column.remove_one(key, true));
        column.frame();
        column.frame(); // Main has neither acknowledged nor extracted a new mask.
    }
    assert!(column.live.is_empty());
    column.frame();
}

#[test]
fn missing_far_replacement_and_retirement_backpressure_preserve_complete_draws() {
    let mut column = Column::new();
    column.far_ready = false;
    Arc::make_mut(&mut column.target.near).clear();
    for _ in 0..3 {
        column.frame();
        assert!(!column.remove_one(column.keys[0], true));
    }
    column.far_ready = true;
    column.frame();
    for key in column.keys {
        assert!(!column.remove_one(key, false));
        column.frame();
        assert!(column.remove_one(key, true));
        column.frame();
    }
}

#[test]
fn incoming_sections_wait_for_one_complete_render_publication_without_main_acknowledgements() {
    let mut column = Column::new();
    Arc::make_mut(&mut column.target.near).clear();
    column.frame();
    for key in column.keys {
        assert!(column.remove_one(key, true));
    }
    column.request_near(3);
    column.frame();
    for (index, key) in column.keys.into_iter().enumerate() {
        column.live.insert(key, 3);
        column.frame();
        assert_eq!(column.state.draws_near(key.chunk()), index == 2);
    }
}

#[test]
fn cancellation_before_or_after_draw_switch_keeps_a_drawer_during_replacement() {
    for switched in [false, true] {
        let mut column = Column::new();
        Arc::make_mut(&mut column.target.near).clear();
        if switched {
            column.frame();
            assert!(column.remove_one(column.keys[0], true));
        }
        column.request_near(3);
        column.frame();
        for key in column.keys {
            column.live.insert(key, 3);
            column.frame();
        }
        assert!(column.state.draws_near(column.keys[0].chunk()));
    }
}

#[test]
fn centre_and_radius_changes_keep_coverage_while_a_new_near_target_is_incomplete() {
    let mut column = Column::new();
    column.target.centre = column.keys[0].chunk();
    column.target.horizon = 2;
    Arc::make_mut(&mut column.target.near).clear();
    column.frame();
    column.target.centre = ChunkKey::new(0, 0, 0);
    column.target.horizon = 8;
    column.request_near(3);
    column.frame();
    for key in column.keys {
        column.live.insert(key, 3);
        column.frame();
    }
}

#[test]
fn stale_extracted_generations_cannot_re_admit_a_partly_removed_column() {
    let mut column = Column::new();
    Arc::make_mut(&mut column.target.near).clear();
    column.frame();
    assert!(column.remove_one(column.keys[0], true));
    column.request_near(1); // An older extraction must not resurrect the old drawer.
    column.frame();
    assert!(!column.state.draws_near(column.keys[0].chunk()));
    column.request_near(3);
    for key in column.keys {
        column.live.insert(key, 3);
    }
    column.frame();
}

#[test]
fn restoring_detail_before_maintenance_cannot_treat_withdrawals_as_empty_near_sections() {
    let mut column = Column::new();
    Arc::make_mut(&mut column.target.near).clear();
    column.frame();
    for key in column.keys {
        assert!(column.remove_one(key, true));
    }
    // The latest radius may request near detail before maintenance replaces old removal jobs.
    column.request_near(2);
    column.frame();
    assert!(!column.state.draws_near(column.keys[0].chunk()));
    column.request_near(3);
    for key in column.keys {
        column.live.insert(key, 3);
        column.frame();
    }
}

#[test]
fn newer_empty_receipts_wait_behind_old_live_geometry_without_being_discarded() {
    let mut column = Column::new();
    column.request_near(2);
    column.state.reset_draws();
    for key in column.keys {
        column.state.publish_empty(key, 2);
    }
    column.frame();
    assert!(!column.state.draws_near(column.keys[0].chunk()));
    column.live.clear();
    column.state.publish(&column.target, &column.live, |_| true);
    assert!(
        column
            .state
            .near_columns()
            .any(|key| key == column.keys[0].chunk())
    );
}

#[test]
fn empty_sections_and_session_replacement_use_current_publications_only() {
    let mut column = Column::new();
    Arc::make_mut(&mut column.target.near).clear();
    column.frame();
    for key in column.keys {
        assert!(column.remove_one(key, true));
    }
    column.request_near(3);
    for key in column.keys {
        column.state.publish_empty(key, 3);
        column.state.withdraw(key, 2);
    }
    column.state.publish(&column.target, &column.live, |_| true);
    assert!(
        column
            .state
            .near_columns()
            .any(|key| key == column.keys[0].chunk())
    );
    // Empty near sections are valid absence, so no visible section should be asserted here.
    column.state = TerrainPublication::default();
    column.state.publish(&column.target, &column.live, |_| true);
    assert!(
        !column
            .state
            .near_columns()
            .any(|key| key == column.keys[0].chunk())
    );
    column
        .live
        .extend(column.keys.into_iter().map(|key| (key, 3)));
    column.frame();
}

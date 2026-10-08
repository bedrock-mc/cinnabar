#[test]
fn start_game_world_time_does_not_seed_prediction_and_replacement_restarts_locally() {
    let position = [0.0, 2.620_01, 0.0];
    let mut ticker = MovementTicker::default();
    let mut physics = LocalPhysicsController::default();
    let mut clock = WorldClock::default();
    let mut weather = WeatherState::default();

    for (expected_session, world_time) in [(1, 0), (2, 123_456_789), (3, i64::MAX), (4, i64::MIN)] {
        replace_session(
            &mut clock,
            &mut weather,
            protocol::WorldEnvironmentBootstrap {
                initial_time: world_time,
                day_cycle_lock_time: 0,
                daylight_cycle_enabled: true,
                weather_cycle_enabled: true,
                rain_level: 0.0,
                lightning_level: 0.0,
            },
            0.0,
        );
        ticker.set_source(MovementSource::FreeCamera);
        reset_start_game_prediction(
            &mut ticker,
            &mut physics,
            clock.session_generation(),
            position,
        );

        assert_eq!(clock.session_generation(), expected_session);
        assert_eq!(
            clock.server_time(),
            Some(0.0),
            "StartGame elapsed world age seeds neither daylight nor movement prediction"
        );
        assert_eq!(ticker.session_generation(), expected_session);
        assert_eq!(ticker.next_tick(), 1);
        assert_eq!(ticker.pending_count(), 0);
        assert_eq!(physics.state().expect("StartGame anchors physics").tick, 0);

        PhysicsAuthorityGate::CandidateEvidence
            .apply_start_game(false, true, &mut ticker, &mut physics)
            .unwrap();
        let discarded =
            physics.advance(Duration::from_millis(50), MovementInput::default(), &Floor);
        assert_eq!(discarded.completed_ticks, 0);
        assert!(discarded.samples.is_empty());

        let first = physics.advance(Duration::from_millis(50), MovementInput::default(), &Floor);
        assert_eq!(first.completed_ticks, 1);
        assert_eq!(first.samples[0].tick, 1);
        ticker
            .enqueue_completed_physics(first.samples[0].clone())
            .unwrap();
        assert_eq!(ticker.pending_snapshots()[0].tick, 1);
    }
}

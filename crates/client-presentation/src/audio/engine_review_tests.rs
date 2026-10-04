use super::tests::engine;
use super::*;

#[test]
fn review_replacing_a_loop_discards_its_pending_start() {
    let mut engine = engine(&[("loop.a", "ambient"), ("loop.b", "ambient")]);
    engine.set_loop(
        "water",
        Some(LoopSpec {
            name: "loop.a".into(),
            volume: 1.0,
        }),
    );
    engine.pending.push(PendingStart {
        request: SoundRequest::new("loop.a"),
        managed: Some(("water", 1.0)),
        roll: [0.0; 3],
        path: "sounds/loop.a".into(),
        category: AudioCategory::Ambient,
        patient: true,
        queued_at: 0.0,
    });
    engine.set_loop(
        "water",
        Some(LoopSpec {
            name: "loop.b".into(),
            volume: 1.0,
        }),
    );
    assert!(engine.pending.is_empty());
    let sources = engine.pump(None, 0.0, &AudioSettings::default());
    assert_eq!(sources.len(), 1);
    assert_eq!(&*engine.voices[0].name, "loop.b");
}

#[test]
fn review_completed_decode_does_not_start_an_expired_one_shot() {
    let mut engine = engine(&[("late", "player")]);
    engine.pending.push(PendingStart {
        request: SoundRequest::new("late"),
        managed: None,
        roll: [0.0; 3],
        path: "sounds/late".into(),
        category: AudioCategory::Players,
        patient: false,
        queued_at: 0.0,
    });
    assert!(
        engine
            .pump(
                None,
                (MAX_DECODE_WAIT_SECONDS + 0.1) as f32,
                &AudioSettings::default()
            )
            .is_empty()
    );
    assert!(engine.pending.is_empty());
}

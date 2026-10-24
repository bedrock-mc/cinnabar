//! Bootstrap authority can present a local body before any received spawn.
use super::*;
use native_tests::{fixture, hand_source_for_mode};

#[test]
fn bootstrap_local_actor_presents_its_canonical_third_person_rig() {
    let (stream, _, _, _) = fixture(None);
    let actor = stream.authority().actor(1).unwrap();
    assert_eq!(actor.spawn_revision, 0, "no local spawn has arrived");
    let rig = stream.authority().actor_rig(1).unwrap();
    let presentation = crate::presentation::actors::actor_rig_presentation(
        &rig,
        actor,
        stream.authority().actor_player_profile(1),
        0.5,
    )
    .expect("the bootstrap actor must retain its canonical rig");
    let batch =
        crate::presentation::actors::select_actor_presentations(1, true, Some(presentation), []);
    assert_eq!(batch.submissions.len(), 1);
    assert_eq!(batch.submissions[0].input.rig.0, rig.rig.0);
    assert!(batch.submissions[0].input.identity.is_exact());
    assert_ne!(
        batch.submissions[0].route,
        render_model::ActorRigRoute::Diagnostic
    );
}

#[test]
fn bootstrap_local_actor_prepares_its_rig_backed_first_person_hand() {
    let (stream, mut equipment, artwork, input) = fixture(None);
    assert_eq!(stream.authority().actor(1).unwrap().spawn_revision, 0);
    let hand = hand_source_for_mode(
        &stream,
        &mut equipment,
        &artwork,
        &input,
        &mut java::HandCache::default(),
        0.5,
        false,
    );
    let body = hand.body.expect("the bootstrap actor must prepare its arm");
    assert!(body.input.identity.is_exact());
    assert_eq!(
        body.input.rig.0,
        stream.authority().actor_rig(1).unwrap().rig.0
    );
}

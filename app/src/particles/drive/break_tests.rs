use super::*;

#[test]
fn missing_local_break_texture_does_not_consume_server_particle_echo() {
    let mut echoes = client_audio::audio::EchoLedger::default();
    let block = [12, 64, -3];

    assert!(!admit_break_echo(
        &mut echoes,
        client_audio::audio::EchoOrigin::Client,
        block,
        0.0,
        false,
    ));
    assert!(admit_break_echo(
        &mut echoes,
        client_audio::audio::EchoOrigin::Packet,
        block,
        0.01,
        true,
    ));
}

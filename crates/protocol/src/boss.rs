use crate::{BossAction, BossEvent, Packet};
use bedrock_protocol_1_26_51::enums::{BossBarColor, BossBarOverlay, BossEventUpdateType};
use bedrock_protocol_1_26_51::packets::BossEvent as BossEventPacket;
use bedrock_protocol_1_26_51::types::ActorUniqueID;
use bedrock_protocol_1_26_51::wire;

/// Acknowledges a changed boss subscription using the boss's unique identity.
#[must_use]
pub fn boss_registration_response(event: &BossEvent) -> Option<Packet> {
    let event_type = match event.action {
        BossAction::Show => BossEventUpdateType::Playeradded,
        BossAction::Hide => BossEventUpdateType::Playerremoved,
        _ => return None,
    };
    let (name, filtered_name) = if event.action == BossAction::Show {
        (
            wire::Str::from(&*event.title),
            wire::Str::from(&*event.filtered_title),
        )
    } else {
        (wire::Str::default(), wire::Str::default())
    };
    Some(crate::Packet::new(BossEventPacket {
        target_actor_id: ActorUniqueID(event.target_entity_id),
        event_type,
        name,
        filtered_name,
        health_percent: wire::F32LE(0.0),
        color: BossBarColor::Blue,
        overlay: BossBarOverlay::Progress,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BossColor, BossOverlay, BossStyle};
    use bedrock_protocol_1_26_51::packets;
    use bedrock_protocol_1_26_51::wire::{Decode, Encode};

    #[test]
    fn subscription_replies_match_the_wire_and_preserve_signed_boss_identity() {
        let mut event = BossEvent {
            target_entity_id: -17,
            action: BossAction::Show,
            title: "Dragon".into(),
            filtered_title: "Filtered".into(),
            progress: 0.75,
            style: BossStyle {
                color: BossColor::Purple,
                overlay: BossOverlay::Notched10,
                darken_sky: None,
                create_world_fog: None,
            },
        };
        for (action, expected) in [
            (
                BossAction::Show,
                &[
                    33, 1, 6, b'D', b'r', b'a', b'g', b'o', b'n', 8, b'F', b'i', b'l', b't', b'e',
                    b'r', b'e', b'd', 0, 0, 0, 0, 1, 0,
                ][..],
            ),
            (BossAction::Hide, &[33, 3, 0, 0, 0, 0, 0, 0, 1, 0][..]),
        ] {
            event.action = action;
            let packet = boss_registration_response(&event).unwrap();
            let packets::Packet::BossEvent(packet) = *packet.data else {
                panic!("wrong response packet");
            };
            let bytes = packet.encode_to_vec().unwrap();
            assert_eq!(bytes, expected);
            let decoded = BossEventPacket::decode_exact(expected).unwrap();
            assert_eq!(decoded.target_actor_id.0, event.target_entity_id);
            assert_eq!(decoded.event_type, packet.event_type);
        }
        event.action = BossAction::SetProgress;
        assert!(boss_registration_response(&event).is_none());
    }
}

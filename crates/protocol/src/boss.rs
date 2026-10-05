use crate::{BossAction, BossEvent, Packet};
use valentine::bedrock::version::v1_26_51::{
    ActorUniqueId, BossEventPacket, EnumsBossBarColor, EnumsBossBarOverlay,
    EnumsBossEventUpdateType,
};

/// Acknowledges a changed boss subscription using the boss's unique identity.
#[must_use]
pub fn boss_registration_response(event: &BossEvent) -> Option<Packet> {
    let event_type = match event.action {
        BossAction::Show => EnumsBossEventUpdateType::Playeradded,
        BossAction::Hide => EnumsBossEventUpdateType::Playerremoved,
        _ => return None,
    };
    let (name, filtered_name) = if event.action == BossAction::Show {
        (event.title.to_string(), event.filtered_title.to_string())
    } else {
        (String::new(), String::new())
    };
    Some(
        BossEventPacket {
            target_actor_id: ActorUniqueId {
                actor_unique_id: event.target_entity_id,
            },
            event_type,
            name,
            filtered_name,
            health_percent: 0.0,
            color: EnumsBossBarColor::Blue,
            overlay: EnumsBossBarOverlay::Progress,
        }
        .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BossColor, BossOverlay, BossStyle};
    use valentine::bedrock::codec::BedrockCodec;

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
            let valentine::bedrock::version::v1_26_51::McpePacketData::BossEventPacket(packet) =
                packet.data
            else {
                panic!("wrong response packet");
            };
            let mut bytes = Vec::new();
            packet.encode(&mut bytes).unwrap();
            assert_eq!(bytes, expected);
            let mut remaining = expected;
            let decoded = BossEventPacket::decode(&mut remaining, ()).unwrap();
            assert!(remaining.is_empty());
            assert_eq!(
                decoded.target_actor_id.actor_unique_id,
                event.target_entity_id
            );
            assert_eq!(decoded.event_type, packet.event_type);
        }
        event.action = BossAction::SetProgress;
        assert!(boss_registration_response(&event).is_none());
    }
}

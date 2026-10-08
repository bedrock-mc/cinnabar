//! Bounded evidence for physical contact, cooldown, progress, and overlay publication.

use std::time::Duration;

use bevy::prelude::{Res, Time};

use super::{
    CameraSettingsAuthority, ScreenEffectFacts,
    facts::portal_body,
    overlay::{OverlayKind, PortalProgress, ScreenOverlays},
};
use crate::local_player::LocalViewPose;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ContactTag {
    session: Option<u64>,
    dimension: Option<i32>,
    touching: bool,
    cooling_down: bool,
    latched: bool,
    texture_visible: bool,
    transfer_active: bool,
}

#[derive(Default)]
pub struct PortalDiagnostics {
    last_tag: Option<ContactTag>,
    last_log: Option<Duration>,
    contact_observed: bool,
    maximum_progress: f32,
}

impl PortalDiagnostics {
    fn due(&mut self, now: Duration, tag: ContactTag, progress: f32) -> bool {
        self.contact_observed |= tag.touching;
        self.maximum_progress = self.maximum_progress.max(progress);
        let changed = self.last_tag != Some(tag);
        let active = self.contact_observed || self.maximum_progress > 0.0 || tag.transfer_active;
        if (!active && !changed)
            || self
                .last_log
                .is_some_and(|last| now.saturating_sub(last) < Duration::from_secs(1))
        {
            return false;
        }
        self.last_tag = Some(tag);
        self.last_log = Some(now);
        true
    }
}

#[allow(clippy::too_many_arguments)]
pub fn diagnose_portal(
    time: Res<Time<bevy::time::Real>>,
    view: Res<LocalViewPose>,
    facts: Res<ScreenEffectFacts>,
    portal: Res<PortalProgress>,
    overlays: Res<ScreenOverlays>,
    settings: Res<CameraSettingsAuthority>,
    physics: Option<&dyn crate::observations::PhysicsObservation>,
    client_world: Option<crate::observations::WorldObservation<'_>>,
    transfer_active: bool,
    diagnostics: &mut PortalDiagnostics,
) {
    if !bevy::log::tracing::enabled!(bevy::log::Level::DEBUG) {
        return;
    }
    let stream = client_world
        .as_ref()
        .and_then(|world| world.stream.as_ref());
    let alpha = overlays
        .layers
        .iter()
        .find(|layer| layer.kind == OverlayKind::Portal)
        .map_or(0.0, |layer| layer.alpha);
    let (cooldown_ticks, latched) = portal.contact_state();
    let tag = ContactTag {
        session: stream.map(|stream| stream.authority().actor_session_id()),
        dimension: stream.map(|stream| stream.current_dimension()),
        touching: facts.in_portal,
        cooling_down: cooldown_ticks != 0,
        latched,
        texture_visible: alpha > 0.0,
        transfer_active,
    };
    if diagnostics.due(time.elapsed(), tag, portal.value()) {
        bevy::log::debug!(
            session = ?tag.session,
            dimension = ?tag.dimension,
            body = ?portal_body(physics, &view),
            eye = ?view.eye_translation(),
            touching = tag.touching,
            contact_observed = diagnostics.contact_observed,
            cooldown_ticks,
            latched,
            progress = portal.value(),
            maximum_progress = diagnostics.maximum_progress,
            alpha,
            perspective = ?settings.perspective(),
            transfer_active = tag.transfer_active,
            confusion = portal.confusion_active,
            "PORTAL_CONTACT"
        );
        diagnostics.contact_observed = false;
        diagnostics.maximum_progress = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(touching: bool) -> ContactTag {
        ContactTag {
            session: Some(1),
            dimension: Some(protocol::NETHER_DIMENSION_ID),
            touching,
            cooling_down: false,
            latched: touching,
            texture_visible: touching,
            transfer_active: false,
        }
    }

    #[test]
    fn short_contacts_survive_rate_limiting_and_quiet_frames_stop_logging() {
        let mut diagnostics = PortalDiagnostics::default();
        assert!(diagnostics.due(Duration::ZERO, tag(false), 0.0));
        assert!(!diagnostics.due(Duration::from_millis(100), tag(true), 0.01));
        assert!(!diagnostics.due(Duration::from_millis(200), tag(false), 0.0));
        assert!(diagnostics.due(Duration::from_secs(1), tag(false), 0.0));
        assert!(diagnostics.contact_observed);
        assert_eq!(diagnostics.maximum_progress, 0.01);
        diagnostics.contact_observed = false;
        diagnostics.maximum_progress = 0.0;
        assert!(!diagnostics.due(Duration::from_secs(10), tag(false), 0.0));
    }

    #[test]
    fn repeated_contact_is_logged_at_most_once_per_second() {
        let mut diagnostics = PortalDiagnostics::default();
        assert!(diagnostics.due(Duration::ZERO, tag(true), 0.01));
        for millis in 1..1000 {
            assert!(!diagnostics.due(Duration::from_millis(millis), tag(true), 0.01));
        }
        assert!(diagnostics.due(Duration::from_secs(1), tag(true), 0.01));
    }
}

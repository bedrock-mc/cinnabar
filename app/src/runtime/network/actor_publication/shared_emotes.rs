//! Maps authenticated API identities onto nearby actors in the current world lifetime.
use actor_animation::CustomEmote;
use bevy::prelude::Time;
use bevy::time::Real;
use bridge::contract::CinnabarEmoteState;
use chunk_pipeline::WorldStream;
use client_presentation::actor_publication::RemoteEmote;
use client_ui::ui_runtime::UiRuntime;
use launcher_host::launcher_account::LauncherAccount;
use std::time::Duration;

const _: () =
    assert!(render_model::MAX_RENDERED_PLAYERS <= bridge::contract::CINNABAR_API_MAX_LOOKUP_XUIDS);

struct Peer {
    identity: String,
    runtime_id: u64,
    spawn_revision: u64,
}

#[derive(Default)]
pub(crate) struct State {
    session: Option<u64>,
    refreshed: Option<Duration>,
    reported: Option<Duration>,
    playback: Option<(CustomEmote, u64)>,
    peers: Vec<Peer>,
}

/// Exchanges only nearby player identities, clearing subscriptions and playback on leaving a world.
pub(super) fn exchange(
    stream: Option<&WorldStream>,
    ui: Option<&UiRuntime>,
    account: Option<&LauncherAccount>,
    time: &Time<Real>,
    state: &mut State,
) -> Vec<RemoteEmote> {
    let Some(account) = account else {
        return Vec::new();
    };
    let session = stream.map(|stream| stream.authority().actor_session_id());
    let now = time.elapsed();
    let changed = session != state.session;
    if changed {
        state.peers.clear();
        state.refreshed = None;
        state.reported = None;
        state.session = session;
    }
    if let Some(stream) = stream
        && state
            .refreshed
            .is_none_or(|last| now.saturating_sub(last) >= Duration::from_millis(500))
    {
        let local = stream
            .authority()
            .actor(stream.local_player_runtime_id())
            .map_or([0.0; 3], |actor| actor.position);
        let mut nearby: Vec<_> = stream
            .authority()
            .remote_actors()
            .filter_map(|actor| {
                let protocol::ActorKind::Player { uuid, .. } = &actor.kind else {
                    return None;
                };
                let profile = stream.authority().actor_player_profile(actor.runtime_id)?;
                let identity = if bridge::valid_cinnabar_xuid(&profile.xuid) {
                    profile.xuid.to_string()
                } else {
                    uuid::Uuid::from_bytes(*uuid).to_string()
                };
                let distance: f32 = actor
                    .position
                    .iter()
                    .zip(local)
                    .map(|(a, b)| (a - b) * (a - b))
                    .sum();
                Some((
                    distance,
                    Peer {
                        identity,
                        runtime_id: actor.runtime_id,
                        spawn_revision: actor.spawn_revision,
                    },
                ))
            })
            .collect();
        nearby.sort_by(|(a, pa), (b, pb)| a.total_cmp(b).then(pa.runtime_id.cmp(&pb.runtime_id)));
        state.peers = nearby
            .into_iter()
            .take(render_model::MAX_RENDERED_PLAYERS)
            .map(|(_, peer)| peer)
            .collect();
        state.refreshed = Some(now);
    }
    let playback = stream.and(ui).and_then(|ui| ui.emotes().playback());
    let key = playback.map(|clip| (clip.emote, clip.revision));
    if changed
        || key != state.playback
        || state
            .reported
            .is_none_or(|last| now.saturating_sub(last) >= Duration::from_millis(100))
    {
        let (mut xuids, mut uuids): (Vec<String>, Vec<String>) = state
            .peers
            .iter()
            .map(|peer| peer.identity.clone())
            .partition(|id| bridge::valid_cinnabar_xuid(id));
        xuids.sort_unstable();
        xuids.dedup();
        uuids.sort_unstable();
        uuids.dedup();
        account.set_cinnabar_emotes(
            session.unwrap_or(0),
            CinnabarEmoteState {
                xuids,
                uuids,
                emote_id: playback.map_or_else(String::new, |clip| clip.emote.id().to_owned()),
                elapsed_ms: playback.map_or(0, |clip| {
                    u32::try_from(
                        u64::try_from(now.as_millis())
                            .unwrap_or(u64::MAX)
                            .saturating_sub(clip.start_millis),
                    )
                    .unwrap_or(u32::MAX)
                }),
                playback_id: playback.map_or(0, |clip| clip.revision),
            },
        );
        state.playback = key;
        state.reported = Some(now);
    }
    let Some(stream) = stream else {
        return Vec::new();
    };
    account
        .cinnabar_emotes()
        .into_iter()
        .filter_map(|emote| {
            let clip = CustomEmote::from_id(&emote.emote_id)?;
            let peer = state
                .peers
                .iter()
                .find(|peer| peer.identity == emote.xuid || peer.identity == emote.uuid)?;
            let actor = stream.authority().actor(peer.runtime_id)?;
            if actor.spawn_revision != peer.spawn_revision
                || actor.status.dead
                || actor.is_sleeping()
            {
                return None;
            }
            Some(RemoteEmote {
                runtime_id: peer.runtime_id,
                spawn_revision: peer.spawn_revision,
                emote: clip,
                elapsed: emote.elapsed_ms as f64 / 1_000.0,
            })
        })
        .collect()
}

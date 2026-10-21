//! Calls a callback's export on a fresh instance of the guest, in the world of its version.

use anyhow::{Result, bail};
use wasmtime::Store;
use wasmtime::component::Resource;

use super::{CallbackRes, Export};
use crate::host::cinnabar::experience_server::types::GuestError;
use crate::host::{HostState, Pre};
use crate::load::Loaded;
use crate::protocol::Op;
use crate::value;

/// Instantiates the guest in `store`, lends it `res` for one export call, and returns the
/// export's result with the ops `res` staged. An error is a trap, or a failure to start.
pub(super) fn invoke(
    store: &mut Store<HostState>,
    loaded: &Loaded,
    res: CallbackRes,
    export: &Export<'_>,
) -> Result<(Result<(), GuestError>, Vec<Op>)> {
    let owned = store.data_mut().table.push(res)?;
    // The guest only borrows the callback, for this call; the host keeps `owned`.
    let ctx = Resource::new_borrow(owned.rep());
    let result = match &loaded.pre {
        Pre::V0_1(pre) => {
            let server = pre.instantiate(&mut *store)?;
            match export {
                Export::Place(change) => server.call_on_place(&mut *store, ctx, change),
                Export::Break(change) => server.call_on_break(&mut *store, ctx, change),
                Export::Interact { player, pos, face } => {
                    server.call_on_interact(&mut *store, ctx, player, *pos, *face)
                }
                Export::Neighbor { pos, neighbor } => {
                    server.call_on_neighbor_changed(&mut *store, ctx, *pos, *neighbor)
                }
                // `run_metered` answers them without running anything.
                Export::ClientMessage { .. } | Export::Epoch { .. } => {
                    bail!("the 0.1 world has neither client-message nor epoch")
                }
                Export::ScheduledUpdate { .. } => bail!("the 0.1 world has no scheduled updates"),
            }
        }
        Pre::V0_2(pre) => {
            let server = pre.instantiate(&mut *store)?;
            match export {
                Export::Place(change) => server.call_on_place(&mut *store, ctx, change),
                Export::Break(change) => server.call_on_break(&mut *store, ctx, change),
                Export::Interact { player, pos, face } => {
                    server.call_on_interact(&mut *store, ctx, player, *pos, *face)
                }
                Export::Neighbor { pos, neighbor } => {
                    server.call_on_neighbor_changed(&mut *store, ctx, *pos, *neighbor)
                }
                Export::ClientMessage {
                    player,
                    channel,
                    schema,
                    payload,
                } => {
                    // `run_metered` answers a payload with lists or records without running it.
                    let Some(leaves) = payload
                        .iter()
                        .map(value::leaf_of)
                        .collect::<Option<Vec<_>>>()
                    else {
                        bail!("the 0.2 world has no list or record values")
                    };
                    server.call_client_message(&mut *store, ctx, player, channel, *schema, &leaves)
                }
                Export::Epoch { .. } => bail!("the 0.2 world has no epoch"),
                Export::ScheduledUpdate { .. } => bail!("the 0.2 world has no scheduled updates"),
            }
        }
        Pre::V0_3(pre) => {
            let server = pre.instantiate(&mut *store)?;
            match export {
                Export::Place(change) => server.call_on_place(&mut *store, ctx, change),
                Export::Break(change) => server.call_on_break(&mut *store, ctx, change),
                Export::Interact { player, pos, face } => {
                    server.call_on_interact(&mut *store, ctx, player, *pos, *face)
                }
                Export::Neighbor { pos, neighbor } => {
                    server.call_on_neighbor_changed(&mut *store, ctx, *pos, *neighbor)
                }
                Export::ClientMessage {
                    player,
                    channel,
                    schema,
                    payload,
                } => {
                    let nodes = value::encode(payload);
                    server.call_client_message(&mut *store, ctx, player, channel, *schema, &nodes)
                }
                Export::Epoch { player } => server.call_epoch(&mut *store, ctx, player),
                Export::ScheduledUpdate { .. } => {
                    bail!("the 0.3 world has no scheduled updates")
                }
            }
        }
        Pre::V0_4(pre) => {
            let server = pre.instantiate(&mut *store)?;
            match export {
                Export::Place(change) => server.call_on_place(&mut *store, ctx, change),
                Export::Break(change) => server.call_on_break(&mut *store, ctx, change),
                Export::Interact { player, pos, face } => {
                    server.call_on_interact(&mut *store, ctx, player, *pos, *face)
                }
                Export::Neighbor { pos, neighbor } => {
                    server.call_on_neighbor_changed(&mut *store, ctx, *pos, *neighbor)
                }
                Export::ClientMessage {
                    player,
                    channel,
                    schema,
                    payload,
                } => {
                    let nodes = value::encode(payload);
                    server.call_client_message(&mut *store, ctx, player, channel, *schema, &nodes)
                }
                Export::Epoch { player } => server.call_epoch(&mut *store, ctx, player),
                Export::ScheduledUpdate { .. } => {
                    bail!("the 0.4 world has no scheduled updates")
                }
            }
        }
        Pre::V0_5(pre) => {
            let server = pre.instantiate(&mut *store)?;
            match export {
                Export::Place(change) => server.call_on_place(&mut *store, ctx, change),
                Export::Break(change) => server.call_on_break(&mut *store, ctx, change),
                Export::Interact { player, pos, face } => {
                    server.call_on_interact(&mut *store, ctx, player, *pos, *face)
                }
                Export::Neighbor { pos, neighbor } => {
                    server.call_on_neighbor_changed(&mut *store, ctx, *pos, *neighbor)
                }
                Export::ClientMessage {
                    player,
                    channel,
                    schema,
                    payload,
                } => {
                    let nodes = value::encode(payload);
                    server.call_client_message(&mut *store, ctx, player, channel, *schema, &nodes)
                }
                Export::Epoch { player } => server.call_epoch(&mut *store, ctx, player),
                Export::ScheduledUpdate { .. } => {
                    bail!("the 0.5 world has no scheduled updates")
                }
            }
        }
        Pre::V0_6(pre) => {
            let server = pre.instantiate(&mut *store)?;
            match export {
                Export::Place(change) => server.call_on_place(&mut *store, ctx, change),
                Export::Break(change) => server.call_on_break(&mut *store, ctx, change),
                Export::Interact { player, pos, face } => {
                    server.call_on_interact(&mut *store, ctx, player, *pos, *face)
                }
                Export::Neighbor { pos, neighbor } => {
                    server.call_on_neighbor_changed(&mut *store, ctx, *pos, *neighbor)
                }
                Export::ClientMessage {
                    player,
                    channel,
                    schema,
                    payload,
                } => {
                    let nodes = value::encode(payload);
                    server.call_client_message(&mut *store, ctx, player, channel, *schema, &nodes)
                }
                Export::Epoch { player } => server.call_epoch(&mut *store, ctx, player),
                Export::ScheduledUpdate { pos } => {
                    server.call_on_scheduled_update(&mut *store, ctx, *pos)
                }
            }
        }
    }?;
    let res = store.data_mut().table.delete(owned)?;
    Ok((result, res.ops))
}

/// What the world of `pre` lacks to run `export`, `focused` when it has its player's focus, if
/// anything: 0.1 has no client-message, 0.2 no list or record values, neither has epoch, only
/// 0.4 and later have a focus, and only 0.6 has on-scheduled-update.
pub(super) fn unsupported(pre: &Pre, export: &Export<'_>, focused: bool) -> Option<&'static str> {
    match (pre, export) {
        (Pre::V0_1(_), Export::ClientMessage { .. }) => Some("no client-message"),
        (Pre::V0_1(_) | Pre::V0_2(_), Export::Epoch { .. }) => Some("no epoch"),
        (Pre::V0_2(_), Export::ClientMessage { payload, .. }) if value::depth(payload) > 0 => {
            Some("no list or record values")
        }
        (Pre::V0_1(_) | Pre::V0_2(_) | Pre::V0_3(_), _) if focused => Some("no focus"),
        (Pre::V0_6(_), _) => None,
        (_, Export::ScheduledUpdate { .. }) => Some("no on-scheduled-update"),
        _ => None,
    }
}

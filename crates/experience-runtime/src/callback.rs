//! One callback: a fresh instance of the guest runs one export against the request's snapshot.
//! What the guest stages through its borrowed `callback` becomes the outcome, or nothing does.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;

use anyhow::{Result, bail};
use serde::Serialize;
use wasmtime::component::Resource;
use wasmtime::{Engine, Store, Trap};

use crate::hex::{self, HexError};
use crate::host::cinnabar::experience_server::types::{
    self as wit, CallbackInfo, ChangeCause, GuestError, ValueNode, WorldError,
};
use crate::host::{HostState, LimitExceeded, Pre};
use crate::limits::{
    CALLBACK_DEADLINE, CALLBACK_FUEL, MAX_BLOCK_DATA_BYTES, MAX_CLIENT_SEND_BYTES,
    MAX_CLIENT_SENDS, MAX_HOST_CALLS, MAX_STAGED_DATA_BYTES, MAX_STAGED_OPS, MAX_TELL_BYTES,
    MAX_TELLS, MAX_VALUE_DEPTH,
};
use crate::load::Loaded;
use crate::protocol::{
    self, BlockPos, Call, Cell, FailKind, Op, Outcome, Request, Scalar, bounded_reason,
};
use crate::value::{self, Refusal};

/// The one block id outside its own namespace that an Experience may place.
const AIR: &str = "minecraft:air";
/// Starts a Minecraft formatting code, so tells may not contain it.
const FORMATTING_PREFIX: char = '§';

/// Runs `request` on a fresh instance of `loaded` under the callback fuel, deadline and store
/// limits. The guest sees the snapshot through a borrowed `callback`, and its staged ops commit
/// only if the export returns `ok`. A request that is not a well-formed callback runs nothing
/// and is rejected.
pub fn run(engine: &Engine, loaded: &Loaded, request: &Request) -> Outcome {
    run_metered(engine, loaded, request).0
}

/// [`run`], which also returns the fuel that the callback consumed.
pub fn run_metered(engine: &Engine, loaded: &Loaded, request: &Request) -> (Outcome, u64) {
    let (res, export) = match prepare(&loaded.block_ids, request) {
        Ok(prepared) => prepared,
        Err(reason) => {
            let reason = format!("malformed callback request: {reason}");
            return (Outcome::Rejected { reason }, 0);
        }
    };
    if let Some(missing) = unsupported(&loaded.pre, &export, res.focus.is_some()) {
        let reason = format!(
            "malformed callback request: api {} has {missing}",
            loaded.manifest.api
        );
        return (Outcome::Rejected { reason }, 0);
    }
    let id = &loaded.manifest.id;
    let mut store = match HostState::store(engine, id, CALLBACK_FUEL, CALLBACK_DEADLINE) {
        Ok(store) => store,
        Err(error) => return (failed(&error), 0),
    };
    let outcome = match invoke(&mut store, loaded, res, &export) {
        Ok((Ok(()), ops)) => Outcome::Committed { ops },
        Ok((Err(GuestError::Rejected(reason) | GuestError::Failed(reason)), _)) => {
            Outcome::Rejected {
                reason: bounded_reason(reason),
            }
        }
        Err(error) => failed(&error),
    };
    // The store meters fuel, so it always reports what is left.
    let fuel = store.get_fuel().map_or(0, |left| CALLBACK_FUEL - left);
    (outcome, fuel)
}

/// Instantiates the guest in `store`, lends it `res` for one export call, and returns the
/// export's result with the ops `res` staged. An error is a trap, or a failure to start.
fn invoke(
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
            }
        }
    }?;
    let res = store.data_mut().table.delete(owned)?;
    Ok((result, res.ops))
}

/// What the world of `pre` lacks to run `export`, `focused` when it has its player's focus, if
/// anything: 0.1 has no client-message, 0.2 no list or record values, neither has epoch, and
/// only 0.4 has a focus.
fn unsupported(pre: &Pre, export: &Export<'_>, focused: bool) -> Option<&'static str> {
    match (pre, export) {
        (Pre::V0_1(_), Export::ClientMessage { .. }) => Some("no client-message"),
        (Pre::V0_1(_) | Pre::V0_2(_), Export::Epoch { .. }) => Some("no epoch"),
        (Pre::V0_2(_), Export::ClientMessage { payload, .. }) if value::depth(payload) > 0 => {
            Some("no list or record values")
        }
        (Pre::V0_1(_) | Pre::V0_2(_) | Pre::V0_3(_), _) if focused => Some("no focus"),
        _ => None,
    }
}

/// The outcome of a callback that trapped or could not start: fuel, the deadline and limits
/// each have their own kind, and anything else is a trap. The reason is the root cause alone,
/// which stays short whatever wraps it.
fn failed(error: &anyhow::Error) -> Outcome {
    let kind = match error.downcast_ref::<Trap>() {
        Some(Trap::OutOfFuel) => FailKind::Fuel,
        Some(Trap::Interrupt) => FailKind::Deadline,
        _ if error.is::<LimitExceeded>() => FailKind::Limit,
        _ => FailKind::Trap,
    };
    Outcome::Failed {
        kind,
        reason: error.root_cause().to_string(),
    }
}

/// The host value behind the guest's `callback` handle: what one callback may see, what it has
/// staged, and how much of its caps it has used.
pub struct CallbackRes {
    info: CallbackInfo,
    actor: Option<String>,
    /// The actor's focus, which a client message or an epoch may have; its snapshot is the one
    /// its anchor would have.
    focus: Option<BlockPos>,
    /// The ids of this Experience's blocks.
    own: Arc<[String]>,
    snapshot: Snapshot,
    /// In the order they commit. A position has at most one `SetBlockData`, and it comes after
    /// any `SetBlock` there.
    ops: Vec<Op>,
    /// Bytes this Experience may still add to its block data.
    budget: u64,
    /// Bytes the staged writes add to this Experience's block data; negative when they free
    /// more than they add.
    added: i64,
    /// Bytes of data in the staged `SetBlockData` ops.
    staged_data: usize,
    host_calls: usize,
    tells: usize,
    client_sends: usize,
    /// Bytes of the staged `SendClient` ops, as [`MAX_CLIENT_SEND_BYTES`] counts them.
    client_send_bytes: usize,
}

/// The snapshot cells by position, with staged writes applied.
struct Snapshot {
    cells: HashMap<BlockPos, Slot>,
    min_y: i32,
    max_y: i32,
    /// The anchor's chunk column, which writes stay inside; a callback without an anchor writes
    /// nothing.
    column: Option<(i32, i32)>,
}

/// One snapshot cell. `owned` means it holds this Experience's block, which alone has data.
struct Slot {
    loaded: bool,
    id: String,
    owned: bool,
    data: Option<Vec<u8>>,
}

/// The export a callback calls, with its arguments.
enum Export<'a> {
    Place(wit::BlockChange),
    Break(wit::BlockChange),
    Interact {
        player: &'a wit::PlayerId,
        pos: wit::BlockPos,
        face: wit::Face,
    },
    Neighbor {
        pos: wit::BlockPos,
        neighbor: wit::BlockPos,
    },
    /// The payload stays in the protocol's form until the world it goes to is known.
    ClientMessage {
        player: &'a wit::PlayerId,
        channel: &'a str,
        schema: u16,
        payload: &'a [Scalar],
    },
    Epoch {
        player: &'a wit::PlayerId,
    },
}

/// The callback's host value and export for `request`, an Experience whose block ids are `own`.
/// The anchor, whose chunk column bounds writes, is the call's position; a client message and an
/// epoch have their player's focus, if any, and without one no snapshot. Hex is decoded and player ids are checked here, so a request
/// that is not a callback, holds bad hex or a player id that is not canonical fails before
/// anything runs.
fn prepare<'a>(
    own: &Arc<[String]>,
    request: &'a Request,
) -> Result<(CallbackRes, Export<'a>), String> {
    let Request::Callback {
        info,
        actor,
        world_min_y,
        world_max_y,
        data_budget,
        snapshot,
        call,
        ..
    } = request
    else {
        return Err("not a callback".to_owned());
    };
    player_id("actor", actor.as_deref())?;
    let mut focused = None;
    let (anchor, export) = match call {
        Call::Place { change } => (Some(change.pos), Export::Place(block_change(change)?)),
        Call::Break { change } => (Some(change.pos), Export::Break(block_change(change)?)),
        Call::Interact { player, pos, face } => {
            player_id("player", Some(player))?;
            let export = Export::Interact {
                player,
                pos: (*pos).into(),
                face: (*face).into(),
            };
            (Some(*pos), export)
        }
        Call::Neighbor { pos, neighbor } => (
            Some(*pos),
            Export::Neighbor {
                pos: (*pos).into(),
                neighbor: (*neighbor).into(),
            },
        ),
        Call::ClientMessage {
            player,
            channel,
            schema,
            payload,
            focus,
        } => {
            player_call(
                "a client message",
                player,
                actor.as_deref(),
                snapshot,
                *focus,
            )?;
            if value::depth(payload) > MAX_VALUE_DEPTH {
                let reason = format!("a client message nests values deeper than {MAX_VALUE_DEPTH}");
                return Err(reason);
            }
            let export = Export::ClientMessage {
                player,
                channel,
                schema: *schema,
                payload,
            };
            focused = *focus;
            (*focus, export)
        }
        Call::Epoch { player, focus } => {
            player_call("an epoch", player, actor.as_deref(), snapshot, *focus)?;
            focused = *focus;
            (*focus, Export::Epoch { player })
        }
    };
    let cells = snapshot
        .iter()
        .map(|cell| {
            let data = decode(cell.data.as_deref())
                .map_err(|error| format!("data of cell {:?}: {error}", cell.pos))?;
            let slot = Slot {
                loaded: cell.loaded,
                id: cell.id.clone(),
                owned: cell.owned,
                data,
            };
            Ok((cell.pos, slot))
        })
        .collect::<Result<_, String>>()?;
    let res = CallbackRes {
        info: CallbackInfo {
            world_id: info.world_id.clone(),
            dimension_id: info.dimension_id.clone(),
            tick: info.tick,
            event_sequence: info.event_sequence,
        },
        actor: actor.clone(),
        focus: focused,
        own: Arc::clone(own),
        snapshot: Snapshot {
            cells,
            min_y: *world_min_y,
            max_y: *world_max_y,
            column: anchor.map(column),
        },
        ops: Vec::new(),
        budget: *data_budget,
        added: 0,
        staged_data: 0,
        host_calls: 0,
        tells: 0,
        client_sends: 0,
        client_send_bytes: 0,
    };
    Ok((res, export))
}

fn block_change(change: &protocol::Change) -> Result<wit::BlockChange, String> {
    player_id("change actor", change.actor.as_deref())?;
    let previous_data = decode(change.previous_data.as_deref())
        .map_err(|error| format!("previous data: {error}"))?;
    Ok(wit::BlockChange {
        pos: change.pos.into(),
        actor: change.actor.clone(),
        cause: change.cause.into(),
        before_id: change.before_id.clone(),
        after_id: change.after_id.clone(),
        previous_data,
    })
}

/// Refuses `id`, named `what`, when it is present but not a canonical player id.
fn player_id(what: &str, id: Option<&str>) -> Result<(), String> {
    match id {
        Some(id) if !protocol::is_player_id(id) => Err(format!(
            "{what} is not a canonical lowercase hyphenated UUID"
        )),
        _ => Ok(()),
    }
}

/// Refuses `what`, a callback for `player` alone, unless `player` is canonical and the actor, and
/// the snapshot is empty without a `focus`.
fn player_call(
    what: &str,
    player: &str,
    actor: Option<&str>,
    snapshot: &[Cell],
    focus: Option<BlockPos>,
) -> Result<(), String> {
    player_id("player", Some(player))?;
    if actor != Some(player) {
        return Err(format!("{what}'s actor is not its player"));
    }
    if focus.is_none() && !snapshot.is_empty() {
        return Err(format!("{what} has a snapshot without a focus"));
    }
    Ok(())
}

fn decode(data: Option<&str>) -> Result<Option<Vec<u8>>, HexError> {
    data.map(hex::decode).transpose()
}

/// The 16×16 chunk column holding `pos`; the arithmetic shift rounds negative coordinates down.
fn column(pos: BlockPos) -> (i32, i32) {
    (pos.x >> 4, pos.z >> 4)
}

impl Snapshot {
    /// A loaded snapshot cell within the world height, which the guest may read.
    fn read(&self, pos: BlockPos) -> Result<&Slot, WorldError> {
        self.within_height(pos)?;
        let slot = self.cells.get(&pos).ok_or(WorldError::Denied)?;
        if slot.loaded {
            Ok(slot)
        } else {
            Err(WorldError::Unavailable)
        }
    }

    /// A cell the guest may read that is also in the anchor's chunk column, so it may write it.
    fn write(&mut self, pos: BlockPos) -> Result<&mut Slot, WorldError> {
        self.within_height(pos)?;
        if self.column != Some(column(pos)) {
            return Err(WorldError::Denied);
        }
        let slot = self.cells.get_mut(&pos).ok_or(WorldError::Denied)?;
        if slot.loaded {
            Ok(slot)
        } else {
            Err(WorldError::Unavailable)
        }
    }

    fn within_height(&self, pos: BlockPos) -> Result<(), WorldError> {
        if (self.min_y..=self.max_y).contains(&pos.y) {
            Ok(())
        } else {
            Err(WorldError::OutOfBounds)
        }
    }
}

/// The world-access methods. Each counts as a host call; the outer error is a trap, and the
/// inner one is a refusal that leaves everything staged as it was.
impl CallbackRes {
    pub(crate) fn info(&mut self) -> Result<CallbackInfo> {
        self.host_call()?;
        Ok(self.info.clone())
    }

    /// The actor's focus, which only a client message or an epoch may have.
    pub(crate) fn focus(&mut self) -> Result<Option<BlockPos>> {
        self.host_call()?;
        Ok(self.focus)
    }

    pub(crate) fn get_block(&mut self, pos: BlockPos) -> Result<Result<String, WorldError>> {
        self.host_call()?;
        Ok(self.snapshot.read(pos).map(|slot| slot.id.clone()))
    }

    /// Replaces air or an own block with air or an own block. The position loses its data, so
    /// data staged for it is dropped, and it is owned exactly when the new block is this
    /// Experience's.
    pub(crate) fn set_block(
        &mut self,
        pos: BlockPos,
        id: String,
    ) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        let slot = match self.snapshot.write(pos) {
            Ok(slot) => slot,
            Err(error) => return Ok(Err(error)),
        };
        if slot.id != AIR && !slot.owned {
            return Ok(Err(WorldError::NotOwned));
        }
        let owned = id != AIR;
        if owned && !self.own.contains(&id) {
            return Ok(Err(WorldError::UnknownBlock));
        }
        if let Some(index) = data_op(&self.ops, pos) {
            self.staged_data -= staged_len(&self.ops.remove(index));
        }
        stage(
            &mut self.ops,
            Op::SetBlock {
                pos,
                id: id.clone(),
            },
        )?;
        self.added -= len(slot.data.as_deref());
        slot.id = id;
        slot.owned = owned;
        slot.data = None;
        Ok(Ok(()))
    }

    pub(crate) fn block_data(
        &mut self,
        pos: BlockPos,
    ) -> Result<Result<Option<Vec<u8>>, WorldError>> {
        self.host_call()?;
        Ok(self.snapshot.read(pos).and_then(|slot| {
            if slot.owned {
                Ok(slot.data.clone())
            } else {
                Err(WorldError::NotOwned)
            }
        }))
    }

    /// Writes or, with `None`, deletes an own block's data, within the size limits and the data
    /// budget. A rewrite replaces the op staged for the block in place.
    pub(crate) fn set_block_data(
        &mut self,
        pos: BlockPos,
        data: Option<Vec<u8>>,
    ) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        let slot = match self.snapshot.write(pos) {
            Ok(slot) => slot,
            Err(error) => return Ok(Err(error)),
        };
        if !slot.owned {
            return Ok(Err(WorldError::NotOwned));
        }
        let size = data.as_ref().map_or(0, Vec::len);
        let earlier = data_op(&self.ops, pos);
        let staged =
            self.staged_data + size - earlier.map_or(0, |index| staged_len(&self.ops[index]));
        if size > MAX_BLOCK_DATA_BYTES || staged > MAX_STAGED_DATA_BYTES {
            return Ok(Err(WorldError::TooLarge));
        }
        let added = self.added + len(data.as_deref()) - len(slot.data.as_deref());
        if u64::try_from(added).is_ok_and(|added| added > self.budget) {
            return Ok(Err(WorldError::QuotaExceeded));
        }
        let op = Op::SetBlockData {
            pos,
            data: data.as_deref().map(hex::encode),
        };
        match earlier {
            Some(index) => self.ops[index] = op,
            None => stage(&mut self.ops, op)?,
        }
        self.staged_data = staged;
        self.added = added;
        slot.data = data;
        Ok(Ok(()))
    }

    /// Tells the event's actor `text`; the tell past [`MAX_TELLS`] traps.
    pub(crate) fn tell(&mut self, player: String, text: String) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        if let Err(error) = self.actor_is(&player) {
            return Ok(Err(error));
        }
        if text.len() > MAX_TELL_BYTES {
            return Ok(Err(WorldError::TooLarge));
        }
        if text
            .chars()
            .any(|c| c.is_control() || c == FORMATTING_PREFIX)
        {
            return Ok(Err(WorldError::InvalidText));
        }
        if self.tells == MAX_TELLS {
            return Err(LimitExceeded(format!("more than {MAX_TELLS} tells")).into());
        }
        stage(&mut self.ops, Op::Tell { player, text })?;
        self.tells += 1;
        Ok(Ok(()))
    }

    /// Stages `payload`, its values' pre-order nodes, for the event's actor's client part on
    /// `channel`. Lists and records nest at most [`MAX_VALUE_DEPTH`] deep, and the callback's
    /// channels and payloads may hold [`MAX_CLIENT_SEND_BYTES`] in all, else it is `too-large`; a
    /// header that counts more items than follow it traps, and so does the send past
    /// [`MAX_CLIENT_SENDS`]. Which channels the client part declares is the adapter's to check.
    pub(crate) fn send_client(
        &mut self,
        player: String,
        channel: String,
        schema: u16,
        payload: Vec<ValueNode>,
    ) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        if let Err(error) = self.actor_is(&player) {
            return Ok(Err(error));
        }
        let payload = match value::decode(payload) {
            Ok(payload) => payload,
            Err(Refusal::TooDeep) => return Ok(Err(WorldError::TooLarge)),
            Err(Refusal::Malformed) => {
                bail!("a send-client list or record counts more items than follow it")
            }
        };
        let bytes = self.client_send_bytes + channel.len() + json_len(&payload);
        if bytes > MAX_CLIENT_SEND_BYTES {
            return Ok(Err(WorldError::TooLarge));
        }
        if self.client_sends == MAX_CLIENT_SENDS {
            let sends = format!("more than {MAX_CLIENT_SENDS} client messages");
            return Err(LimitExceeded(sends).into());
        }
        let op = Op::SendClient {
            player,
            channel,
            schema,
            payload,
        };
        stage(&mut self.ops, op)?;
        self.client_sends += 1;
        self.client_send_bytes = bytes;
        Ok(Ok(()))
    }

    /// Whether `player` may be told or sent to: only the event's actor may.
    fn actor_is(&self, player: &str) -> Result<(), WorldError> {
        match &self.actor {
            None => Err(WorldError::PlayerUnavailable),
            Some(actor) if actor != player => Err(WorldError::Denied),
            Some(_) => Ok(()),
        }
    }

    /// Counts one host call; the call past [`MAX_HOST_CALLS`] traps.
    fn host_call(&mut self) -> Result<()> {
        if self.host_calls == MAX_HOST_CALLS {
            return Err(LimitExceeded(format!("more than {MAX_HOST_CALLS} host calls")).into());
        }
        self.host_calls += 1;
        Ok(())
    }
}

/// Appends `op`; the op past [`MAX_STAGED_OPS`] traps.
fn stage(ops: &mut Vec<Op>, op: Op) -> Result<()> {
    if ops.len() == MAX_STAGED_OPS {
        return Err(LimitExceeded(format!("more than {MAX_STAGED_OPS} staged ops")).into());
    }
    ops.push(op);
    Ok(())
}

/// The index of the staged `SetBlockData` at `pos`.
fn data_op(ops: &[Op], pos: BlockPos) -> Option<usize> {
    ops.iter()
        .position(|op| matches!(op, Op::SetBlockData { pos: at, .. } if *at == pos))
}

/// The bytes of data that a staged op writes; its hex has two digits per byte.
fn staged_len(op: &Op) -> usize {
    match op {
        Op::SetBlockData {
            data: Some(hex), ..
        } => hex.len() / 2,
        _ => 0,
    }
}

/// The length of some data; absent data has none. A slice holds at most `isize::MAX` bytes, so
/// the length fits.
fn len(data: Option<&[u8]>) -> i64 {
    data.map_or(0, |data| data.len() as i64)
}

/// The bytes of `value` as JSON, counted without building the JSON.
fn json_len(value: &impl Serialize) -> usize {
    struct Count(usize);
    impl io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 += bytes.len();
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count(0);
    serde_json::to_writer(&mut count, value).expect("protocol values serialize");
    count.0
}

impl From<BlockPos> for wit::BlockPos {
    fn from(BlockPos { x, y, z }: BlockPos) -> Self {
        Self { x, y, z }
    }
}

impl From<wit::BlockPos> for BlockPos {
    fn from(wit::BlockPos { x, y, z }: wit::BlockPos) -> Self {
        Self { x, y, z }
    }
}

impl From<protocol::Face> for wit::Face {
    fn from(face: protocol::Face) -> Self {
        match face {
            protocol::Face::Down => Self::Down,
            protocol::Face::Up => Self::Up,
            protocol::Face::North => Self::North,
            protocol::Face::South => Self::South,
            protocol::Face::West => Self::West,
            protocol::Face::East => Self::East,
        }
    }
}

impl From<protocol::Cause> for ChangeCause {
    fn from(cause: protocol::Cause) -> Self {
        match cause {
            protocol::Cause::Player => Self::Player,
            protocol::Cause::Guest => Self::Guest,
            protocol::Cause::Environment => Self::Environment,
        }
    }
}

#[cfg(test)]
mod tests;

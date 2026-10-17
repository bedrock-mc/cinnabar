//! The host side of server WIT 0.5, which artifacts built before 0.6 target. 0.6 only added to
//! 0.5's types, so the 0.5 world shares the current WIT's types, and its imports act exactly like
//! the current ones. A 0.5 guest has no `on-scheduled-update` and stages no update.

use anyhow::{Result, bail};
use wasmtime::component::Resource;

use super::HostState;
use super::cinnabar::experience_server::types::{
    BlockState, CallbackInfo, Inventory, LogLevel, Network, NewStack, WorldError,
};
use super::world_access::HostCallback as Current;
use crate::callback::CallbackRes;

wasmtime::component::bindgen!({
    path: "wit/0.5",
    world: "server",
    imports: { default: trappable },
    with: {
        "cinnabar:experience-server/types": crate::host::cinnabar::experience_server::types,
        "cinnabar:experience-server/world-access/callback": crate::callback::CallbackRes,
    },
});

use cinnabar::experience_server::{diagnostics, world_access};

/// The 0.5 WIT that `bindgen!` reads; its `package` line names the version.
pub(crate) const WIT: &str = include_str!("../../wit/0.5/server.wit");

impl diagnostics::Host for HostState {
    fn log(&mut self, level: LogLevel, text: String) -> Result<()> {
        super::diagnostics::Host::log(self, level, text)
    }
}

impl world_access::Host for HostState {}

/// Each method is the current world's; see [`CallbackRes`] for the rules.
impl world_access::HostCallback for HostState {
    fn info(&mut self, ctx: Resource<CallbackRes>) -> Result<CallbackInfo> {
        Current::info(self, ctx)
    }

    fn get_block(
        &mut self,
        ctx: Resource<CallbackRes>,
        pos: BlockPos,
    ) -> Result<Result<String, WorldError>> {
        Current::get_block(self, ctx, pos)
    }

    fn set_block(
        &mut self,
        ctx: Resource<CallbackRes>,
        pos: BlockPos,
        id: String,
    ) -> Result<Result<(), WorldError>> {
        Current::set_block(self, ctx, pos, id)
    }

    fn block_data(
        &mut self,
        ctx: Resource<CallbackRes>,
        pos: BlockPos,
    ) -> Result<Result<Option<Vec<u8>>, WorldError>> {
        Current::block_data(self, ctx, pos)
    }

    fn set_block_data(
        &mut self,
        ctx: Resource<CallbackRes>,
        pos: BlockPos,
        data: Option<Vec<u8>>,
    ) -> Result<Result<(), WorldError>> {
        Current::set_block_data(self, ctx, pos, data)
    }

    fn tell(
        &mut self,
        ctx: Resource<CallbackRes>,
        player: String,
        text: String,
    ) -> Result<Result<(), WorldError>> {
        Current::tell(self, ctx, player, text)
    }

    fn send_client(
        &mut self,
        ctx: Resource<CallbackRes>,
        player: String,
        channel: String,
        schema: u16,
        payload: Vec<ValueNode>,
    ) -> Result<Result<(), WorldError>> {
        Current::send_client(self, ctx, player, channel, schema, payload)
    }

    fn focus(&mut self, ctx: Resource<CallbackRes>) -> Result<Option<BlockPos>> {
        Current::focus(self, ctx)
    }

    fn block_states(
        &mut self,
        ctx: Resource<CallbackRes>,
        pos: BlockPos,
    ) -> Result<Result<Vec<BlockState>, WorldError>> {
        Current::block_states(self, ctx, pos)
    }

    fn set_block_state(
        &mut self,
        ctx: Resource<CallbackRes>,
        pos: BlockPos,
        states: Vec<BlockState>,
    ) -> Result<Result<(), WorldError>> {
        Current::set_block_state(self, ctx, pos, states)
    }

    fn network(
        &mut self,
        ctx: Resource<CallbackRes>,
    ) -> Result<Result<Option<Network>, WorldError>> {
        Current::network(self, ctx)
    }

    fn inventory(&mut self, ctx: Resource<CallbackRes>) -> Result<Result<Inventory, WorldError>> {
        Current::inventory(self, ctx)
    }

    fn set_slot(
        &mut self,
        ctx: Resource<CallbackRes>,
        slot: u32,
        stack: Option<NewStack>,
    ) -> Result<Result<(), WorldError>> {
        Current::set_slot(self, ctx, slot, stack)
    }

    fn drop_item(
        &mut self,
        ctx: Resource<CallbackRes>,
        pos: BlockPos,
        stack: NewStack,
    ) -> Result<Result<(), WorldError>> {
        Current::drop_item(self, ctx, pos, stack)
    }

    /// Only reachable for an owned handle, and the guest is only ever lent a callback.
    fn drop(&mut self, _: Resource<CallbackRes>) -> Result<()> {
        bail!("the guest dropped a callback it cannot own")
    }
}

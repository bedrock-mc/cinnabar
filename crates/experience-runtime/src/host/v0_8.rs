//! The preserved server WIT 0.8 host. Its existing types share the current implementation;
//! It delegates shared imports while retaining the frozen export contract.

use anyhow::{Result, bail};
use wasmtime::component::Resource;

use super::HostState;
use super::cinnabar::experience_server::types::{
    self as wit, BlockState, CallbackInfo, Crafted, Inventory, ItemKey, LogLevel, Network,
    NewStack, WorldError,
};
use super::world_access::HostCallback as Current;
use crate::callback::CallbackRes;

wasmtime::component::bindgen!({
    path: "wit/0.8",
    world: "server",
    imports: { default: trappable },
    with: {
        "cinnabar:experience-server/types": crate::host::cinnabar::experience_server::types,
        "cinnabar:experience-server/world-access/callback": crate::callback::CallbackRes,
    },
});

use cinnabar::experience_server::{diagnostics, world_access};

/// The 0.8 WIT that `bindgen!` reads; its `package` line names the version.
pub(crate) const WIT: &str = include_str!("../../wit/0.8/server.wit");

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

    fn world(&mut self, ctx: Resource<CallbackRes>) -> Result<wit::WorldHandle> {
        self.table.get_mut(&ctx)?.world()
    }
    fn player_location(
        &mut self,
        ctx: Resource<CallbackRes>,
        player: String,
    ) -> Result<Result<wit::PlayerLocation, WorldError>> {
        self.table.get_mut(&ctx)?.player_location(player)
    }
    fn resolve_world(
        &mut self,
        ctx: Resource<CallbackRes>,
        id: wit::WorldInstanceId,
    ) -> Result<Result<wit::WorldHandle, WorldError>> {
        self.table.get_mut(&ctx)?.resolve_world(id)
    }
    fn create_world(
        &mut self,
        ctx: Resource<CallbackRes>,
        instance: wit::WorldInstance,
    ) -> Result<Result<(), WorldError>> {
        self.table.get_mut(&ctx)?.create_world(instance)
    }
    fn unload_world(
        &mut self,
        ctx: Resource<CallbackRes>,
        instance: wit::WorldHandle,
    ) -> Result<Result<(), WorldError>> {
        self.table.get_mut(&ctx)?.unload_world(instance)
    }
    fn transfer(
        &mut self,
        ctx: Resource<CallbackRes>,
        player: String,
        instance: wit::WorldHandle,
        position: wit::Position,
    ) -> Result<Result<(), WorldError>> {
        self.table
            .get_mut(&ctx)?
            .transfer(player, instance, position)
    }
    fn store_read(
        &mut self,
        ctx: Resource<CallbackRes>,
        key: wit::StoreKey,
    ) -> Result<Result<Option<Vec<u8>>, WorldError>> {
        self.table.get_mut(&ctx)?.store_read(key)
    }
    fn store_write(
        &mut self,
        ctx: Resource<CallbackRes>,
        key: wit::StoreKey,
        data: Option<Vec<u8>>,
    ) -> Result<Result<(), WorldError>> {
        self.table.get_mut(&ctx)?.store_write(key, data)
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

    /// Retains replies from guests built against the frozen world API.
    fn send_control(
        &mut self,
        ctx: Resource<CallbackRes>,
        player: String,
        channel: String,
        schema: u16,
        payload: Vec<wit::ValueNode>,
    ) -> Result<Result<(), WorldError>> {
        Current::send_control(self, ctx, player, channel, schema, payload)
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

    fn crafting_result(
        &mut self,
        ctx: Resource<CallbackRes>,
        grid: Vec<Option<ItemKey>>,
    ) -> Result<Option<Crafted>> {
        Current::crafting_result(self, ctx, grid)
    }
    fn schedule_update(
        &mut self,
        ctx: Resource<CallbackRes>,
        pos: BlockPos,
        ticks: u32,
    ) -> Result<Result<(), WorldError>> {
        Current::schedule_update(self, ctx, pos, ticks)
    }

    /// Only reachable for an owned handle, and the guest is only ever lent a callback.
    fn drop(&mut self, _: Resource<CallbackRes>) -> Result<()> {
        bail!("the guest dropped a callback it cannot own")
    }
}

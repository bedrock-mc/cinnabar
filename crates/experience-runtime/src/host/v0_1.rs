//! The host side of server WIT 0.1, which artifacts built before 0.2 target. Later versions only
//! added to 0.1's types, so the 0.1 world shares the current WIT's types and its imports act
//! exactly like theirs.

use anyhow::{Result, bail};
use wasmtime::component::Resource;

use super::HostState;
use super::cinnabar::experience_server::types::{CallbackInfo, LogLevel, WorldError};
use crate::callback::CallbackRes;

wasmtime::component::bindgen!({
    path: "wit/0.1",
    world: "server",
    imports: { default: trappable },
    with: {
        "cinnabar:experience-server/types": crate::host::cinnabar::experience_server::types,
        "cinnabar:experience-server/world-access/callback": crate::callback::CallbackRes,
    },
});

use cinnabar::experience_server::{diagnostics, world_access};

/// The 0.1 WIT that `bindgen!` reads; its `package` line names the version.
pub(crate) const WIT: &str = include_str!("../../wit/0.1/server.wit");

impl diagnostics::Host for HostState {
    fn log(&mut self, level: LogLevel, text: String) -> Result<()> {
        super::diagnostics::Host::log(self, level, text)
    }
}

impl world_access::Host for HostState {}

/// Each method is the current world's; see [`CallbackRes`] for the rules.
impl world_access::HostCallback for HostState {
    fn info(&mut self, ctx: Resource<CallbackRes>) -> Result<CallbackInfo> {
        self.table.get_mut(&ctx)?.info()
    }

    fn get_block(
        &mut self,
        ctx: Resource<CallbackRes>,
        pos: BlockPos,
    ) -> Result<Result<String, WorldError>> {
        self.table.get_mut(&ctx)?.get_block(pos.into())
    }

    fn set_block(
        &mut self,
        ctx: Resource<CallbackRes>,
        pos: BlockPos,
        id: String,
    ) -> Result<Result<(), WorldError>> {
        self.table.get_mut(&ctx)?.set_block(pos.into(), id)
    }

    fn block_data(
        &mut self,
        ctx: Resource<CallbackRes>,
        pos: BlockPos,
    ) -> Result<Result<Option<Vec<u8>>, WorldError>> {
        self.table.get_mut(&ctx)?.block_data(pos.into())
    }

    fn set_block_data(
        &mut self,
        ctx: Resource<CallbackRes>,
        pos: BlockPos,
        data: Option<Vec<u8>>,
    ) -> Result<Result<(), WorldError>> {
        self.table.get_mut(&ctx)?.set_block_data(pos.into(), data)
    }

    fn tell(
        &mut self,
        ctx: Resource<CallbackRes>,
        player: String,
        text: String,
    ) -> Result<Result<(), WorldError>> {
        self.table.get_mut(&ctx)?.tell(player, text)
    }

    /// Only reachable for an owned handle, and the guest is only ever lent a callback.
    fn drop(&mut self, _: Resource<CallbackRes>) -> Result<()> {
        bail!("the guest dropped a callback it cannot own")
    }
}

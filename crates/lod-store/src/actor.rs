//! The write-back actor: one background thread that owns a [`Store`] and is its only
//! writer.
//!
//! Requests travel through a bounded channel and never block the sender: a full channel
//! hands the request back. The thread drains requests in batches and coalesces writes per
//! key, chunk and surface, so the last write of a batch wins. Reads of section records and
//! chunk content hashes see every write sent before them and are answered through a
//! bounded result queue; each read takes a slot until its result is polled, and every
//! read gets exactly one result, a failure included, even when the thread stops. Flushes
//! run when the caller sends [`Request::Flush`] on its periodic cadence, each compacting
//! within a byte budget so queued reads wait little; syncs to disk when it sends
//! [`Request::Sync`] as the client goes idle; and quota enforcement, which walks every
//! store, when it sends [`Request::EnforceQuotas`], each with the caller's time. Closing
//! flushes, enforces the quotas and syncs; [`Closing`] reports when the store's lock is
//! released, so the same store can be opened again as its writer. Neither closing nor
//! dropping the handle ever blocks its thread. A [`StoreClient`] lets background threads
//! submit requests and wait for reads of their own until the handle closes.
//!
//! Every session change bumps a [`SessionEpoch`]: a begun or ended session as soon as its
//! request is queued, and a candidate that merges, becomes established or is dropped when
//! the thread decides it. Writes prepared from what one epoch held are sent with it
//! ([`StoreClient::write_in_epoch`]) and refused when the thread has seen a change since, so
//! terrain merged from one epoch's sections never lands in another.

mod client;
mod closing;
mod handle;
mod messages;
#[cfg(test)]
mod tests;
mod view;
mod worker;

#[cfg(any(test, feature = "test-support"))]
pub use self::handle::Paused;
pub use self::{
    client::StoreClient,
    closing::Closing,
    handle::StoreActor,
    messages::{
        ActorConfig, ActorEvent, EpochWrite, ReadOutcome, ReadResult, ReadTarget, ReadTicket,
        Request, SessionEpoch, SubmitError,
    },
    view::{KEPT_HIDDEN_REGIONS, ViewChange, ViewEpoch},
};
use self::{handle::ShutdownTime, messages::Command};

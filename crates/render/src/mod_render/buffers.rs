//! Buffer growth that keeps retiring storage within its bundle's signed allowance.

use bevy::render::{
    render_resource::{Buffer, BufferDescriptor},
    renderer::{RenderDevice, RenderQueue},
};
use mod_render::{GpuAllocation, GpuLedger};

/// Replaces storage when admitted. If only the replacement fits, retires the old buffer
/// and retries after GPU completion. Callers must clear bindings before calling this.
pub(super) fn replace(
    current: &mut Option<GpuAllocation<Buffer>>,
    descriptor: &BufferDescriptor<'_>,
    ledger: Option<&GpuLedger>,
    device: &RenderDevice,
    queue: &RenderQueue,
) -> bool {
    let replacement = GpuAllocation::new(ledger, descriptor.size, || {
        crate::gpu_allocation::buffer(device, descriptor)
    });
    let retire = replacement.is_some()
        || ledger.is_some_and(|ledger| {
            current.as_ref().is_some_and(|old| {
                ledger
                    .used()
                    .saturating_sub(old.size())
                    .checked_add(descriptor.size)
                    .is_some_and(|bytes| bytes <= ledger.limit())
            })
        });
    if retire && let Some(old) = current.take() {
        crate::device_poll::on_frame_complete(queue, move || drop(old));
    }
    let replaced = replacement.is_some();
    if replaced {
        *current = replacement;
    }
    replaced
}

use crate::chunk::*;

pub(super) fn prepare_publication_removals(
    arena: &mut ChunkGpuArena,
    budget: ChunkUploadBudget,
    gpu_removals: &ChunkGpuRemovalQueue,
    acknowledgements: &ChunkUploadAcknowledgements,
) {
    let maximum_zero_byte_operations = budget
        .max_zero_byte_operations_per_frame
        .min(PublicationServiceConfig::PHASE2_GATE.maximum_zero_byte_operations_per_frame);
    let mut zero_byte_operations = 0;
    let mut physically_removed_keys = HashSet::new();
    for entity in arena.pending_removals.iter().copied().collect::<Vec<_>>() {
        let Some(allocation) = arena.allocations.get(&entity).cloned() else {
            arena.pending_removals.remove(&entity);
            continue;
        };
        if zero_byte_operations >= maximum_zero_byte_operations {
            break;
        }
        if allocation.liquid_range.is_none() {
            physically_removed_keys.insert(allocation.gpu.key);
            free_allocation(arena, entity);
            arena.pending_removals.remove(&entity);
            zero_byte_operations = zero_byte_operations.saturating_add(1);
            continue;
        }
        let allocation_key = allocation.gpu.key;
        let retirement = RetiredArenaAllocation::full(entity, allocation);
        let bytes = retirement.owned_bytes();
        if !arena.retirement_budget.can_reserve(1, bytes) {
            continue;
        }
        arena
            .allocations
            .remove(&entity)
            .expect("pending removal retains its arena allocation");
        physically_removed_keys.insert(allocation_key);
        assert!(arena.retirement_budget.try_reserve(1, bytes));
        arena.retired_allocations.push(retirement);
        arena.pending_removals.remove(&entity);
        zero_byte_operations = zero_byte_operations.saturating_add(1);
    }

    let completed_removals = gpu_removals.take_ready(maximum_zero_byte_operations, |key| {
        if physically_removed_keys.contains(&key) {
            return true;
        }
        if arena
            .allocations
            .values()
            .any(|allocation| allocation.gpu.key == key)
        {
            return false;
        }
        if zero_byte_operations >= maximum_zero_byte_operations {
            return false;
        }
        zero_byte_operations = zero_byte_operations.saturating_add(1);
        true
    });
    for pending in completed_removals {
        if pending
            .token
            .is_some_and(|token| !acknowledgements.try_reserve(pending.key, token))
        {
            gpu_removals
                .push(pending)
                .unwrap_or_else(|_| unreachable!("taking one removal frees one mailbox slot"));
            continue;
        }
        let permit = match pending.permit.into_gpu_prepared() {
            Ok(permit) => permit,
            Err(permit) => {
                if let Some(token) = pending.token {
                    acknowledgements.cancel(pending.key, token);
                }
                drop(permit);
                continue;
            }
        };
        if let Some(token) = pending.token {
            acknowledgements.complete(pending.key, token, Instant::now());
        }
        let retired = permit.retire();
        debug_assert!(retired);
    }
}

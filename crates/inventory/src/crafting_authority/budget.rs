use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicUsize, Ordering},
};

const MAX_RETAINED_BYTES: usize = 8 * 1024 * 1024;

/// Requested-allocation accounting only; no gameplay state or reconnect reset.
#[derive(Debug)]
pub(super) struct Credits {
    maximum: usize,
    used: AtomicUsize,
}

#[derive(Debug)]
pub(super) struct Permit {
    owner: Arc<Credits>,
    bytes: usize,
}

impl Credits {
    pub(super) fn shared() -> Arc<Self> {
        static OWNER: OnceLock<Arc<Credits>> = OnceLock::new();
        Arc::clone(OWNER.get_or_init(|| {
            Arc::new(Self {
                maximum: MAX_RETAINED_BYTES,
                used: AtomicUsize::new(0),
            })
        }))
    }

    pub(super) fn reserve(self: &Arc<Self>, bytes: usize) -> Option<Permit> {
        self.used
            .try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|next| *next <= self.maximum)
            })
            .ok()?;
        Some(Permit {
            owner: Arc::clone(self),
            bytes,
        })
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.owner.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_credit_refusal_releases_all_cells_without_partial_publication() {
        use super::super::projection::StackOwner;
        use protocol::{
            ContainerIdentity, InventoryEvent, InventorySlotEvent, InventoryTransactionEvent,
            NetworkItemStack, SlotIdentity,
        };
        let credits = Arc::new(Credits {
            maximum: MAX_RETAINED_BYTES,
            used: AtomicUsize::new(0),
        });
        let stack = NetworkItemStack::empty();
        let charge = std::mem::size_of::<StackOwner>() + stack.extra_data.len() + 64;
        let held = credits.reserve(MAX_RETAINED_BYTES - charge).unwrap();
        let update = |slot| InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity {
                    window_id: Some(protocol::UI_INVENTORY_WINDOW_ID),
                    slot_type: Some(0),
                    dynamic_id: None,
                },
                slot,
            },
            stack: stack.clone(),
            storage_item: None,
        };
        let event = crate::InventoryAuthorityEvent::Inventory(InventoryEvent::Transaction(
            InventoryTransactionEvent {
                slots: Arc::from([update(28), update(29)]),
                skipped_actions: 0,
            },
        ));
        let before = Arc::strong_count(&stack.extra_data);
        let mut state = super::super::CraftingAuthority::with_credits(1, Arc::clone(&credits));
        state.observe(1, 1, &event);
        assert!(state.queue.is_none());
        assert!(state.grid.iter().all(Option::is_none));
        assert!(state.cursor.is_none());
        assert_eq!(state.barrier, 1);
        assert_eq!(
            credits.used.load(Ordering::Acquire),
            MAX_RETAINED_BYTES - charge
        );
        assert_eq!(Arc::strong_count(&stack.extra_data), before);
        drop(held);
        assert_eq!(credits.used.load(Ordering::Acquire), 0);
    }

    #[test]
    fn complete_grid_reserves_before_cloning_and_releases_all_four_permits_once() {
        use super::super::projection::StackOwner;
        let credits = Arc::new(Credits {
            maximum: MAX_RETAINED_BYTES,
            used: AtomicUsize::new(0),
        });
        let stack = protocol::NetworkItemStack {
            extra_data: Arc::from([0u8; 10]),
            ..protocol::NetworkItemStack::empty()
        };
        let charge = std::mem::size_of::<StackOwner>() + stack.extra_data.len() + 64;
        let held = credits.reserve(MAX_RETAINED_BYTES - charge * 3).unwrap();
        let before = Arc::strong_count(&stack.extra_data);
        assert!(StackOwner::grid_with_credits([&stack; 4], &credits).is_none());
        assert_eq!(
            Arc::strong_count(&stack.extra_data),
            before,
            "no clone before fourth permit"
        );
        assert_eq!(
            credits.used.load(Ordering::Acquire),
            MAX_RETAINED_BYTES - charge * 3
        );
        drop(held);
        let grid = StackOwner::grid_with_credits([&stack; 4], &credits).unwrap();
        assert_eq!(credits.used.load(Ordering::Acquire), charge * 4);
        let old = grid.clone();
        drop(grid);
        assert_eq!(credits.used.load(Ordering::Acquire), charge * 4);
        assert_eq!(Arc::strong_count(&stack.extra_data), before + 4);
        drop(old);
        assert_eq!(credits.used.load(Ordering::Acquire), 0);
        assert_eq!(Arc::strong_count(&stack.extra_data), before);
    }

    #[test]
    fn grid_queue_refusal_rolls_back_owners_and_keeps_ordinary_drain() {
        use super::super::projection::StackOwner;
        let credits = Arc::new(Credits {
            maximum: MAX_RETAINED_BYTES,
            used: AtomicUsize::new(0),
        });
        let stack = protocol::NetworkItemStack::empty();
        let charge = std::mem::size_of::<StackOwner>() + stack.extra_data.len() + 64;
        // Four owners fit exactly, but the separately charged queue cannot.
        let held = credits.reserve(MAX_RETAINED_BYTES - charge * 4).unwrap();
        let mut runtime = crate::InventorySession::new(1);
        runtime.crafting_authority =
            super::super::CraftingAuthority::with_credits(1, Arc::clone(&credits));
        let event = protocol::InventoryEvent::Content(protocol::InventoryContentEvent {
            container: protocol::ContainerIdentity {
                window_id: Some(124),
                slot_type: Some(0),
                dynamic_id: None,
            },
            slots: vec![stack; 54].into(),
            storage_item: protocol::NetworkItemStack::empty(),
        });
        runtime.enqueue_inventory_event(1, 1, event).unwrap();
        runtime.synchronize_crafting_frontier(1, Some((1, 0, Some(1))));
        runtime.drain_pending_inventory();
        assert!(runtime.pending_inventory.is_empty());
        assert!(runtime.crafting_authority.queue.is_none());
        assert!(runtime.crafting_authority.grid.iter().all(Option::is_none));
        assert_eq!(runtime.crafting_authority.barrier, 1);
        assert_eq!(
            credits.used.load(Ordering::Acquire),
            MAX_RETAINED_BYTES - charge * 4
        );
        drop(runtime);
        drop(held);
        assert_eq!(credits.used.load(Ordering::Acquire), 0);
    }

    #[test]
    fn immutable_owners_keep_credit_until_final_drop_and_refusal_cannot_remint() {
        let credits = Arc::new(Credits {
            maximum: 8,
            used: AtomicUsize::new(0),
        });
        let owner = Arc::new(credits.reserve(8).unwrap());
        let clone = Arc::clone(&owner);
        assert!(credits.reserve(1).is_none());
        drop(owner);
        assert_eq!(credits.used.load(Ordering::Acquire), 8);
        drop(clone);
        assert_eq!(credits.used.load(Ordering::Acquire), 0);
        assert!(credits.reserve(8).is_some());
    }

    #[test]
    fn registry_and_stack_owners_hold_original_allocations_and_release_exactly_once() {
        use super::super::projection::{RegistryOwner, StackOwner};
        let credits = Arc::new(Credits {
            maximum: 1024 * 1024,
            used: AtomicUsize::new(0),
        });
        let event = protocol::ItemRegistryEvent {
            entries: Arc::from([protocol::ItemRegistryEntry {
                identifier: Arc::from("minecraft:oak_log"),
                network_id: 6,
                component_based: false,
                version: protocol::ItemRegistryVersion::None,
                component_digest: [0; 32],
                negotiated_max_stack_size: Some(64),
                canonical_empty_component_data: true,
                item_tags: std::sync::Arc::from([]),
            }]),
        };
        let owner =
            RegistryOwner::with_credits(&event, std::num::NonZeroU64::new(1).unwrap(), &credits)
                .unwrap();
        assert!(std::ptr::eq(
            owner.snapshot.get(6).unwrap(),
            &event.entries[0]
        ));
        let charged = credits.used.load(Ordering::Acquire);
        assert!(charged > event.entries[0].identifier.len());
        let clone = Arc::clone(&owner);
        drop(owner);
        assert_eq!(credits.used.load(Ordering::Acquire), charged);
        drop(clone);
        assert_eq!(credits.used.load(Ordering::Acquire), 0);
        let stack =
            StackOwner::with_credits(&protocol::NetworkItemStack::empty(), &credits).unwrap();
        let clone = Arc::clone(&stack);
        drop(stack);
        assert!(credits.used.load(Ordering::Acquire) > 0);
        drop(clone);
        assert_eq!(credits.used.load(Ordering::Acquire), 0);
    }

    #[test]
    fn refusal_precedes_registry_retention_and_invalid_index_releases_its_reservation() {
        use super::super::projection::RegistryOwner;
        let event = protocol::ItemRegistryEvent {
            entries: protocol::vanilla_item_registry(),
        };
        let no_credit = Arc::new(Credits {
            maximum: 0,
            used: AtomicUsize::new(0),
        });
        let before = Arc::strong_count(&event.entries);
        assert!(
            RegistryOwner::with_credits(&event, std::num::NonZeroU64::new(1).unwrap(), &no_credit)
                .is_none()
        );
        assert_eq!(Arc::strong_count(&event.entries), before);
        assert_eq!(no_credit.used.load(Ordering::Acquire), 0);
        let credits = Arc::new(Credits {
            maximum: 1024 * 1024,
            used: AtomicUsize::new(0),
        });
        let duplicate = protocol::ItemRegistryEvent {
            entries: Arc::from([event.entries[0].clone(), event.entries[0].clone()]),
        };
        assert!(
            RegistryOwner::with_credits(
                &duplicate,
                std::num::NonZeroU64::new(1).unwrap(),
                &credits
            )
            .is_none()
        );
        assert_eq!(credits.used.load(Ordering::Acquire), 0);
    }

    #[test]
    fn real_retention_refusal_preserves_ordinary_ingress_drain_and_clone_credits() {
        let credits = Arc::new(Credits {
            maximum: MAX_RETAINED_BYTES,
            used: AtomicUsize::new(0),
        });
        let mut runtime = crate::InventorySession::new(1);
        runtime.crafting_authority =
            super::super::CraftingAuthority::with_credits(1, Arc::clone(&credits));
        runtime.synchronize_crafting_frontier(1, Some((1, 0, Some(0))));
        // These unique names/counts fit the existing registry admission policy.
        // Two held immutable indexes exceed the actual 8 MiB requested-allocation
        // policy without synthetic allocator faults or changing ordinary queues.
        let prefix = "x".repeat(240);
        let registry = protocol::ItemRegistryEvent {
            entries: (1..=protocol::MAX_ITEM_REGISTRY_ENTRIES)
                .map(|id| protocol::ItemRegistryEntry {
                    identifier: format!("test:{prefix}{id:05}").into(),
                    network_id: i32::try_from(id).unwrap(),
                    component_based: false,
                    version: protocol::ItemRegistryVersion::None,
                    component_digest: [0; 32],
                    negotiated_max_stack_size: Some(64),
                    canonical_empty_component_data: true,
                    item_tags: std::sync::Arc::from([]),
                })
                .collect(),
        };
        let route = |runtime: &mut crate::InventorySession, sequence| {
            runtime
                .enqueue_item_registry_event(1, sequence, registry.clone())
                .unwrap();
            runtime.drain_pending_inventory();
        };
        route(&mut runtime, 1);
        assert_eq!(
            runtime
                .crafting_authority
                .queue
                .as_ref()
                .unwrap()
                .records
                .len(),
            1
        );
        assert!(runtime.pending_inventory.is_empty());
        let charged = credits.used.load(Ordering::Acquire);
        assert!(charged > MAX_RETAINED_BYTES / 2 && charged < MAX_RETAINED_BYTES);
        let old = runtime.clone();
        assert_eq!(
            credits.used.load(Ordering::Acquire),
            charged,
            "clone shares index/queue allocations and their permits"
        );
        route(&mut runtime, 2);
        assert!(
            runtime.pending_inventory.is_empty(),
            "ordinary drain retains its original destructive timing"
        );
        assert_eq!(
            runtime.inventory_ledger.negotiated_item_entry(1),
            Some(&registry.entries[0])
        );
        assert_eq!(
            runtime
                .inventory_ledger
                .negotiated_item_entry(protocol::MAX_ITEM_REGISTRY_ENTRIES as i32),
            registry.entries.last()
        );
        assert!(runtime.crafting_authority.queue.is_none());
        assert_eq!(runtime.crafting_authority.barrier, 2);
        assert!(runtime.crafting_authority.registry.is_none());
        assert_eq!(
            credits.used.load(Ordering::Acquire),
            charged,
            "refusal cannot release an older clone's retained permits"
        );
        drop(old);
        assert_eq!(
            credits.used.load(Ordering::Acquire),
            0,
            "final clone releases queue/index credits exactly once"
        );
        route(&mut runtime, 3);
        runtime.synchronize_crafting_frontier(1, Some((1, 0, Some(3))));
        runtime.drain_pending_inventory();
        assert!(
            runtime.crafting_authority.registry.is_some(),
            "fresh post-barrier replacement can recover binding"
        );
        assert!(
            runtime.crafting_preview().is_none(),
            "registry recovery never invents cursor/grid facts"
        );
        drop(runtime);
        assert_eq!(credits.used.load(Ordering::Acquire), 0);
    }

    #[test]
    fn production_construction_and_reconnect_always_share_the_same_owner() {
        let mut runtime = crate::InventorySession::new(1);
        let old = runtime.clone();
        runtime.begin_session(2);
        assert!(Arc::ptr_eq(
            &runtime.crafting_authority.credits,
            &old.crafting_authority.credits
        ));
        assert!(Arc::ptr_eq(
            &Credits::shared(),
            &runtime.crafting_authority.credits
        ));
    }

    #[test]
    fn an_old_real_preview_retains_all_credit_until_its_final_runtime_drop() {
        use protocol::{ContainerIdentity, InventoryEvent, InventorySlotEvent, SlotIdentity};
        let credits = Arc::new(Credits {
            maximum: MAX_RETAINED_BYTES,
            used: AtomicUsize::new(0),
        });
        let mut runtime = crate::InventorySession::new(1);
        runtime.crafting_authority =
            super::super::CraftingAuthority::with_credits(1, Arc::clone(&credits));
        assert!(runtime.publish_bootstrap_inventory(
            Some(protocol::ItemRegistryEvent {
                entries: protocol::vanilla_item_registry(),
            }),
            InventoryEvent::Authority(protocol::InventoryAuthority::Server),
        ));
        let mut body = [0; 12];
        body[11] = 1;
        runtime
            .enqueue_inventory_event(
                1,
                1,
                InventoryEvent::Recipes(protocol::decode_recipe_update(&body).unwrap()),
            )
            .unwrap();
        for (sequence, name, slot) in [
            (2, 13, 28),
            (3, 13, 29),
            (4, 13, 30),
            (5, 13, 31),
            (6, 59, 0),
        ] {
            runtime
                .enqueue_inventory_event(
                    1,
                    sequence,
                    InventoryEvent::Slot(InventorySlotEvent {
                        identity: SlotIdentity {
                            container: ContainerIdentity {
                                window_id: Some(124),
                                slot_type: Some(name),
                                dynamic_id: None,
                            },
                            slot,
                        },
                        stack: protocol::NetworkItemStack::empty(),
                        storage_item: None,
                    }),
                )
                .unwrap();
        }
        runtime.synchronize_crafting_frontier(1, Some((1, 0, Some(6))));
        runtime.drain_pending_inventory();
        assert_eq!(
            runtime.crafting_preview(),
            Some(super::super::CraftingPreview::NoMatch)
        );
        assert!(runtime.pending_inventory.is_empty());
        let charged = credits.used.load(Ordering::Acquire);
        assert!(charged > 0);
        let old = runtime.clone();
        assert!(Arc::ptr_eq(
            runtime.crafting_authority.preview.as_ref().unwrap(),
            old.crafting_authority.preview.as_ref().unwrap(),
        ));
        assert_eq!(credits.used.load(Ordering::Acquire), charged);
        runtime.begin_session(2);
        assert!(runtime.crafting_preview().is_none());
        assert_eq!(
            old.crafting_preview(),
            Some(super::super::CraftingPreview::NoMatch)
        );
        assert_eq!(credits.used.load(Ordering::Acquire), charged);
        drop(runtime);
        assert_eq!(credits.used.load(Ordering::Acquire), charged);
        drop(old);
        assert_eq!(credits.used.load(Ordering::Acquire), 0);
    }
    #[test]
    fn registry_credits_include_retained_item_tags() {
        use super::super::projection::RegistryOwner;
        let credits = Arc::new(Credits {
            maximum: 1024,
            used: AtomicUsize::new(0),
        });
        let mut entry = protocol::vanilla_item_registry()[0].clone();
        entry.item_tags = Arc::from([Arc::from("x".repeat(2048))]);
        let event = protocol::ItemRegistryEvent {
            entries: Arc::from([entry]),
        };
        assert!(
            RegistryOwner::with_credits(&event, std::num::NonZeroU64::new(1).unwrap(), &credits)
                .is_none()
        );
        assert_eq!(credits.used.load(Ordering::Acquire), 0);
    }
}

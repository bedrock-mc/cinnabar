//! Book icons resolve once per change of the listed stacks or an icon source.

use std::{cell::Cell, sync::Arc};

use super::*;

fn stack(network_id: i32) -> protocol::NetworkItemStack {
    protocol::NetworkItemStack {
        network_id,
        count: 1,
        ..protocol::NetworkItemStack::empty()
    }
}

#[test]
fn unchanged_entries_reuse_icons_until_a_stack_or_source_changes() {
    let mut cache = BookIconCache::default();
    let calls = Cell::new(0);
    let icon = |stack: &protocol::NetworkItemStack| {
        calls.set(calls.get() + 1);
        Some(IconRef {
            color_mask: None,
            page: 0,
            uv: [u16::try_from(stack.network_id).unwrap(), 0, 16, 16],
            glint: false,
        })
    };
    let stacks = || (1..=1500).map(stack).collect::<Vec<_>>();
    let registry = Some(Arc::new(()));
    let sources = || BookIconSources {
        registry: registry.clone(),
        ..BookIconSources::default()
    };
    let first = cache.icons(stacks(), sources(), icon);
    // The publisher asks every frame; an unchanged book resolves nothing again.
    for _ in 0..30 {
        assert!(Arc::ptr_eq(&first, &cache.icons(stacks(), sources(), icon)));
    }
    assert_eq!(calls.get(), 1500);
    let mut fewer = stacks();
    fewer.pop();
    assert_eq!(cache.icons(fewer.clone(), sources(), icon).len(), 1499);
    let moved = BookIconSources {
        dials: inventory::DialFrames {
            clock: Some(3),
            ..Default::default()
        },
        ..sources()
    };
    cache.icons(fewer.clone(), moved, icon);
    let replaced = BookIconSources {
        registry: Some(Arc::new(())),
        ..sources()
    };
    cache.icons(fewer, replaced, icon);
    assert_eq!(cache.resolutions, 4);
}

//! Bounded ownership for connections read by native crash handlers.

use std::{
    ops::Deref,
    sync::{
        Mutex,
        atomic::{AtomicPtr, AtomicUsize, Ordering},
    },
};

/// One current value and at most one retired value while a crash handler may still read it.
pub(super) struct Slot<T> {
    current: AtomicPtr<T>,
    readers: AtomicUsize,
    retired: Mutex<Option<Box<T>>>,
}

impl<T> Slot<T> {
    /// Starts without a published connection.
    pub(super) const fn new() -> Self {
        Self {
            current: AtomicPtr::new(std::ptr::null_mut()),
            readers: AtomicUsize::new(0),
            retired: Mutex::new(None),
        }
    }

    /// Borrows the current value using only atomics, including on a signal handler's stack.
    pub(super) fn read(&self) -> Option<Guard<'_, T>> {
        self.readers.fetch_add(1, Ordering::SeqCst);
        let pointer = self.current.load(Ordering::SeqCst);
        if pointer.is_null() {
            self.readers.fetch_sub(1, Ordering::SeqCst);
            None
        } else {
            Some(Guard {
                slot: self,
                pointer,
            })
        }
    }

    /// Replaces the connection outside crash handlers, refusing a third live allocation.
    pub(super) fn publish(&self, value: T) -> bool {
        let mut retired = self
            .retired
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if retired.is_some() && self.readers.load(Ordering::SeqCst) != 0 {
            return false;
        }
        drop(retired.take());
        let old = self
            .current
            .swap(Box::into_raw(Box::new(value)), Ordering::SeqCst);
        if !old.is_null() {
            // SAFETY: A swapped pointer is retired exactly once under this publisher lock.
            let old = unsafe { Box::from_raw(old) };
            if self.readers.load(Ordering::SeqCst) != 0 {
                *retired = Some(old);
            }
        }
        true
    }
}

impl<T> Drop for Slot<T> {
    /// Releases the current value after exclusive ownership proves that no guard remains.
    fn drop(&mut self) {
        let pointer = *self.current.get_mut();
        if !pointer.is_null() {
            // SAFETY: An exclusive slot borrow outlives all guards.
            drop(unsafe { Box::from_raw(pointer) });
        }
    }
}

/// A signal-safe borrow; dropping it only updates the reader count.
pub(crate) struct Guard<'a, T> {
    slot: &'a Slot<T>,
    pointer: *const T,
}

impl<T> Deref for Guard<'_, T> {
    type Target = T;
    /// Reads the value whose lifetime is protected by this guard's reader reservation.
    fn deref(&self) -> &T {
        // SAFETY: Publishers keep an observed value retired while any guard exists.
        unsafe { &*self.pointer }
    }
}

impl<T> Drop for Guard<'_, T> {
    /// Ends a signal handler's borrow without locks or allocation.
    fn drop(&mut self) {
        self.slot.readers.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn replacements_reclaim_connections_and_protect_active_readers() {
        let slot = Slot::new();
        let value = Arc::new(7);
        for _ in 0..1024 {
            assert!(slot.publish(Arc::clone(&value)));
        }
        assert_eq!(Arc::strong_count(&value), 2);
        let old = slot.read().unwrap();
        assert!(slot.publish(Arc::new(8)));
        assert_eq!(**old, 7);
        assert!(
            !slot.publish(Arc::new(9)),
            "a live reader must bound retired connections"
        );
        drop(old);
        assert!(slot.publish(Arc::new(10)));
        assert_eq!(Arc::strong_count(&value), 1);
        assert_eq!(**slot.read().unwrap(), 10);
    }
}

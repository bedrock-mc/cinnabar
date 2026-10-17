/// Whether typed item data forbids dropping, destroying or consuming this stack.
pub(crate) fn special_removal_locked(extra: &[u8]) -> bool {
    protocol::item_extra_lock(extra).is_some_and(|lock| lock != 0)
}

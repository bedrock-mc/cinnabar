//! Lock-free rings a crashed process leaves readable: each lives in process memory until it is
//! attached to a [`MappedRegion`] in the session's staging directory, after which every record
//! goes straight into the mapped file, where another process reads it after any crash.
//!
//! A ring file is little-endian 64-bit words: a magic word, a layout word (`version | capacity
//! << 32`), ring-specific header words up to [`HEADER_WORDS`], then fixed-size slots. Each slot
//! starts with a stamp, `sequence << 8 | state`, that only moves forward: a writer claims a slot
//! by stamping it [`WRITING`], fills it, then publishes it; a writer whose record a newer one
//! already replaced, or that finds an older writer still filling the slot, gives its record up
//! rather than mixing two. Readers keep a record only if its stamp is the same published one
//! before and after reading it.

pub mod flight;
pub mod packets;
mod region;

use std::{
    io,
    path::Path,
    sync::atomic::{AtomicPtr, AtomicU64, Ordering, fence},
};

use region::MappedRegion;

// Ring files are little-endian words, and the rings store native atomics.
const _: () = assert!(cfg!(target_endian = "little"));

/// Words before a ring's first slot.
pub const HEADER_WORDS: usize = 8;
/// The state byte of a stamp whose slot is being written; no published state uses it.
pub const WRITING: u8 = 0xff;
const MAGIC_WORD: usize = 0;
const LAYOUT_WORD: usize = 1;

/// A ring's words: in this struct until [`Words::attach`] moves them into a mapped file.
pub(crate) struct Words<const N: usize> {
    inline: [AtomicU64; N],
    mapped: AtomicPtr<AtomicU64>,
}

impl<const N: usize> Words<N> {
    pub(crate) const fn new() -> Self {
        Self {
            inline: [const { AtomicU64::new(0) }; N],
            mapped: AtomicPtr::new(std::ptr::null_mut()),
        }
    }

    /// The words records go to now.
    #[inline]
    pub(crate) fn get(&self) -> &[AtomicU64; N] {
        let mapped = self.mapped.load(Ordering::Acquire);
        if mapped.is_null() {
            &self.inline
        } else {
            // SAFETY: `attach` only publishes a leaked region of exactly `N` words.
            unsafe { &*mapped.cast::<[AtomicU64; N]>() }
        }
    }

    /// Maps a new `N`-word file at `path`, copies the records so far into it, stamps it with
    /// `magic` and `layout`, and sends every later record there for the rest of the process.
    /// A record written during the copy may be lost. Fails when the file cannot be mapped or
    /// the ring is already attached, leaving any file of an attached ring untouched.
    pub(crate) fn attach(&self, path: &Path, magic: u64, layout: u64) -> io::Result<()> {
        if !self.mapped.load(Ordering::Acquire).is_null() {
            return Err(already_attached());
        }
        let region = MappedRegion::create(path, N)?;
        let target = region.words();
        for (from, to) in self.inline.iter().zip(target).skip(LAYOUT_WORD + 1) {
            to.store(from.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        target[MAGIC_WORD].store(magic, Ordering::Relaxed);
        target[LAYOUT_WORD].store(layout, Ordering::Relaxed);
        let region = Box::into_raw(Box::new(region));
        // SAFETY: Just allocated; leaked for the process unless the exchange below fails.
        let words = unsafe { (*region).words().as_ptr().cast_mut() };
        match self.mapped.compare_exchange(
            std::ptr::null_mut(),
            words,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Ok(()),
            Err(_) => {
                // SAFETY: Never published, so this is the only owner.
                drop(unsafe { Box::from_raw(region) });
                Err(already_attached())
            }
        }
    }
}

fn already_attached() -> io::Error {
    io::Error::new(io::ErrorKind::AlreadyExists, "the ring is already attached")
}

/// The layout word of a ring at `version` with `capacity` slots.
pub(crate) const fn layout(version: u32, capacity: usize) -> u64 {
    version as u64 | (capacity as u64) << 32
}

/// Reads a ring file image of `N` words, checking its magic and layout words.
pub(crate) fn image<const N: usize>(
    bytes: &[u8],
    magic: u64,
    layout: u64,
) -> io::Result<Box<[AtomicU64]>> {
    let invalid = |reason: &str| io::Error::new(io::ErrorKind::InvalidData, reason.to_owned());
    if bytes.len() != N * 8 {
        return Err(invalid("the ring file has the wrong size"));
    }
    let words: Box<[AtomicU64]> = bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|word| AtomicU64::new(u64::from_le_bytes(*word)))
        .collect();
    if words[MAGIC_WORD].load(Ordering::Relaxed) != magic {
        return Err(invalid("the file is not this kind of ring"));
    }
    if words[LAYOUT_WORD].load(Ordering::Relaxed) != layout {
        return Err(invalid("the ring file has another version or capacity"));
    }
    Ok(words)
}

/// Claims `stamp`'s slot for record `sequence`, unless a newer record owns it or an older writer
/// is still filling it. Only a claimed record may be published.
#[inline]
pub(crate) fn claim(stamp: &AtomicU64, sequence: u64) -> bool {
    let mut current = stamp.load(Ordering::Relaxed);
    loop {
        if current >> 8 >= sequence || current & 0xff == u64::from(WRITING) {
            return false;
        }
        match stamp.compare_exchange_weak(
            current,
            sequence << 8 | u64::from(WRITING),
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(actual) => current = actual,
        }
    }
    // Readers that see any field written after this also see the slot as being written.
    fence(Ordering::Release);
    true
}

/// The stamp of published record `sequence` in `state`.
#[inline]
pub(crate) const fn published(sequence: u64, state: u8) -> u64 {
    sequence << 8 | state as u64
}

/// The state of record `sequence` if `stamp` holds it published.
#[inline]
pub(crate) fn published_state(stamp: u64, sequence: u64) -> Option<u8> {
    (stamp >> 8 == sequence && stamp & 0xff != u64::from(WRITING)).then_some(stamp as u8)
}

/// Sequence numbers a ring of `capacity` slots still holds after `recorded` records, oldest
/// first.
pub(crate) fn retained(recorded: u64, capacity: usize) -> std::ops::RangeInclusive<u64> {
    recorded.saturating_sub(capacity as u64) + 1..=recorded
}

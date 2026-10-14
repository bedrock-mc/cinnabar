//! One frame's writes to the renderer's buffers, staged for one upload and one scatter
//! dispatch: commands that copy staged words, or fill a run with one word, into a target.

use super::layout::{
    COMMAND_WORDS, FILL_FLAG, MAX_COMMAND_WORDS, MAX_COMMANDS, STAGING_HEADER_WORDS, Target,
};

/// The first staged payload word, after the header and every command slot.
pub(crate) const PAYLOAD_WORD: u32 = STAGING_HEADER_WORDS + MAX_COMMANDS * COMMAND_WORDS;

/// The staged writes of one frame; see the module documentation.
#[derive(Debug, Default)]
pub(crate) struct Staging {
    /// The command count, then reserved zeros.
    header: [u32; STAGING_HEADER_WORDS as usize],
    commands: Vec<[u32; COMMAND_WORDS as usize]>,
    payload: Vec<u32>,
    /// Payload words the staging buffer holds.
    payload_capacity: usize,
}

impl Staging {
    /// Staging for a buffer of `words` words.
    pub(crate) fn new(words: u64) -> Self {
        Self {
            header: [0; STAGING_HEADER_WORDS as usize],
            commands: Vec::new(),
            payload: Vec::new(),
            payload_capacity: words.saturating_sub(u64::from(PAYLOAD_WORD)) as usize,
        }
    }

    /// Commands one write of `words` words needs.
    pub(crate) fn commands_for(words: usize) -> usize {
        words.div_ceil(MAX_COMMAND_WORDS as usize)
    }

    /// True when `commands` more commands and `words` more payload words fit.
    pub(crate) fn has_room(&self, commands: usize, words: usize) -> bool {
        self.commands.len() + commands <= MAX_COMMANDS as usize
            && self.payload.len() + words <= self.payload_capacity
    }

    /// Stages a copy of `words` to word `dst` of `target`; false, staging nothing, when it
    /// does not fit.
    pub(crate) fn copy(&mut self, target: Target, dst: u32, words: &[u32]) -> bool {
        self.copy_with(target, dst, words.len(), |payload| {
            payload.extend_from_slice(words);
        })
    }

    /// Stages a copy of `len` words that `write` appends to the payload, to word `dst` of
    /// `target`; false, calling nothing, when it does not fit.
    pub(crate) fn copy_with(
        &mut self,
        target: Target,
        dst: u32,
        len: usize,
        write: impl FnOnce(&mut Vec<u32>),
    ) -> bool {
        if len == 0 {
            return true;
        }
        if !self.has_room(Self::commands_for(len), len) {
            return false;
        }
        let source = PAYLOAD_WORD + self.payload.len() as u32;
        write(&mut self.payload);
        debug_assert_eq!(
            self.payload.len() as u32 - (source - PAYLOAD_WORD),
            len as u32
        );
        for offset in (0..len as u32).step_by(MAX_COMMAND_WORDS as usize) {
            let run = (len as u32 - offset).min(MAX_COMMAND_WORDS);
            self.commands
                .push([target as u32, dst + offset, source + offset, run]);
        }
        self.header[0] = self.command_count();
        true
    }

    /// Stages filling words `dst..dst + len` of `target` with `value`; false, staging
    /// nothing, when it does not fit.
    pub(crate) fn fill(&mut self, target: Target, dst: u32, len: u32, value: u32) -> bool {
        if len == 0 {
            return true;
        }
        if !self.has_room(Self::commands_for(len as usize), 0) {
            return false;
        }
        for offset in (0..len).step_by(MAX_COMMAND_WORDS as usize) {
            let run = (len - offset).min(MAX_COMMAND_WORDS);
            self.commands
                .push([target as u32, dst + offset, value, run | FILL_FLAG]);
        }
        self.header[0] = self.command_count();
        true
    }

    /// Commands staged.
    pub(crate) fn command_count(&self) -> u32 {
        self.commands.len() as u32
    }

    /// Payload bytes staged.
    pub(crate) fn payload_bytes(&self) -> u64 {
        self.payload.len() as u64 * 4
    }

    /// True when nothing is staged.
    pub(crate) fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Bytes of the staging buffer's prefix that [`Self::write`] fills.
    pub(crate) fn written_bytes(&self) -> u64 {
        if self.payload.is_empty() {
            u64::from(STAGING_HEADER_WORDS + self.command_count() * COMMAND_WORDS) * 4
        } else {
            (u64::from(PAYLOAD_WORD) + self.payload.len() as u64) * 4
        }
    }

    /// The header, the commands and the payload with the byte offsets they go to in the
    /// staging buffer, which together fill its first [`Self::written_bytes`] bytes; empty
    /// parts are left out.
    pub(crate) fn parts(&self) -> impl Iterator<Item = (usize, &[u8])> {
        [
            (0, bytemuck::cast_slice(&self.header)),
            (
                STAGING_HEADER_WORDS as usize * 4,
                bytemuck::cast_slice(&self.commands),
            ),
            (
                PAYLOAD_WORD as usize * 4,
                bytemuck::cast_slice(&self.payload),
            ),
        ]
        .into_iter()
        .filter(|(_, bytes): &(usize, &[u8])| !bytes.is_empty())
    }

    /// Forgets everything staged.
    pub(crate) fn clear(&mut self) {
        self.header[0] = 0;
        self.commands.clear();
        self.payload.clear();
    }

    /// What the scatter kernel does with the staged writes, on CPU-side copies of the
    /// targets indexed by [`Target`]; for tests.
    #[cfg(test)]
    pub(crate) fn apply(&self, targets: &mut [Vec<u32>; 5]) {
        for &[target, dst, source, len] in &self.commands {
            let words = &mut targets[target as usize];
            let run = (len & !FILL_FLAG) as usize;
            let dst = dst as usize;
            if words.len() < dst + run {
                words.resize(dst + run, 0);
            }
            if len & FILL_FLAG != 0 {
                words[dst..dst + run].fill(source);
            } else {
                let source = (source - PAYLOAD_WORD) as usize;
                words[dst..dst + run].copy_from_slice(&self.payload[source..source + run]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_writes_split_into_bounded_commands_and_round_trip() {
        let mut staging = Staging::new(u64::from(PAYLOAD_WORD) + 10_000);
        let words: Vec<u32> = (0..5_000).collect();
        assert!(staging.copy(Target::Pool, 10, &words));
        assert!(staging.fill(Target::Flags, 3, 4_097, 9));
        assert_eq!(staging.command_count(), 3 + 3);
        let mut targets: [Vec<u32>; 5] = Default::default();
        staging.apply(&mut targets);
        assert_eq!(&targets[Target::Pool as usize][10..], &words[..]);
        assert!(
            targets[Target::Flags as usize][3..]
                .iter()
                .all(|&word| word == 9)
        );
        assert_eq!(targets[Target::Flags as usize].len(), 3 + 4_097);
        let mut out = vec![0_u8; staging.written_bytes() as usize];
        for (offset, bytes) in staging.parts() {
            out[offset..offset + bytes.len()].copy_from_slice(bytes);
        }
        let written: &[u32] = bytemuck::cast_slice(&out);
        assert_eq!(written[0], 6);
        assert_eq!(written[PAYLOAD_WORD as usize + 4_999], 4_999);
    }

    #[test]
    fn writes_that_do_not_fit_stage_nothing() {
        let mut staging = Staging::new(u64::from(PAYLOAD_WORD) + 8);
        assert!(!staging.copy(Target::Order, 0, &[1; 9]));
        assert!(staging.is_empty());
        assert!(staging.copy(Target::Order, 0, &[1; 8]));
        assert!(!staging.copy(Target::Order, 8, &[1]));
        assert_eq!(staging.payload_bytes(), 32);
        staging.clear();
        assert!(staging.is_empty());
    }
}

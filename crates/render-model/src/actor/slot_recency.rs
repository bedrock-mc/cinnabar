//! Constant-work recency for stable render slots, with no allocation on warm frames.

#[derive(Clone, Copy, Debug)]
struct Link {
    previous: Option<usize>,
    next: Option<usize>,
    frame: u64,
}

/// The oldest slot is always at the head; slots touched this frame cannot be evicted.
#[derive(Debug, Default)]
pub struct FrameSlotRecency {
    links: Vec<Option<Link>>,
    head: Option<usize>,
    tail: Option<usize>,
}

impl FrameSlotRecency {
    /// Admits or moves a stable slot to the newest end without searching other slots.
    pub fn touch(&mut self, slot: usize, frame: u64) {
        if self
            .links
            .get(slot)
            .and_then(Option::as_ref)
            .is_some_and(|link| link.frame == frame)
        {
            return;
        }
        self.remove(slot);
        if self.links.len() <= slot {
            self.links.resize(slot + 1, None);
        }
        self.links[slot] = Some(Link {
            previous: self.tail,
            next: None,
            frame,
        });
        if let Some(tail) = self.tail {
            self.links[tail].as_mut().expect("retained tail").next = Some(slot);
        } else {
            self.head = Some(slot);
        }
        self.tail = Some(slot);
    }

    /// Removes one admission before its slot receives new content.
    pub fn remove(&mut self, slot: usize) {
        let Some(link) = self.links.get_mut(slot).and_then(Option::take) else {
            return;
        };
        if let Some(previous) = link.previous {
            self.links[previous]
                .as_mut()
                .expect("retained predecessor")
                .next = link.next;
        } else {
            self.head = link.next;
        }
        if let Some(next) = link.next {
            self.links[next]
                .as_mut()
                .expect("retained successor")
                .previous = link.previous;
        } else {
            self.tail = link.previous;
        }
    }

    /// A single head check protects every slot already selected in the current frame.
    pub fn oldest_unused(&self, frame: u64) -> Option<usize> {
        let head = self.head?;
        (self.links[head].as_ref()?.frame != frame).then_some(head)
    }
}

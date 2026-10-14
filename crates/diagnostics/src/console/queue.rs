//! The bounded queue between console writers and the writer thread. Writes carry a
//! [`Priority`]: normal writes fill only the shared capacity, so a flood of them can never take
//! the reserve kept for warnings and errors, and an error waits a bounded time for space.
use super::{Message, Priority};
use std::{
    collections::VecDeque,
    sync::{Condvar, Mutex, MutexGuard},
    time::{Duration, Instant},
};

/// Writes of any priority queued before normal writes are dropped.
pub(super) const SHARED_CAPACITY: usize = 4096;
/// Further writes only warnings and errors may use.
pub(super) const RESERVED_CAPACITY: usize = 1024;
/// Longest an error waits for space once even the reserve is full.
pub(super) const ERROR_WAIT: Duration = Duration::from_millis(50);

/// Time source and timed wait for producers; tests substitute one that never blocks.
pub(super) trait Clock: Send + Sync {
    fn now(&self) -> Instant;
    /// Waits on `space` until notified or `deadline`, whichever is first; may return early.
    fn wait<'a>(
        &self,
        space: &Condvar,
        state: MutexGuard<'a, State>,
        deadline: Instant,
    ) -> MutexGuard<'a, State>;
}

/// The process clock.
pub(super) struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn wait<'a>(
        &self,
        space: &Condvar,
        state: MutexGuard<'a, State>,
        deadline: Instant,
    ) -> MutexGuard<'a, State> {
        let timeout = deadline.saturating_duration_since(Instant::now());
        space
            .wait_timeout(state, timeout)
            .map_or_else(|error| error.into_inner().0, |(state, _)| state)
    }
}

pub(super) struct State {
    items: VecDeque<(Message, bool)>,
    /// Queued messages that count against capacity.
    writes: usize,
    closed: bool,
}

/// Why a message was not queued; the message comes back to the caller.
pub(super) enum Rejected {
    Full(Message),
    Closed(Message),
}

pub(super) struct Queue {
    state: Mutex<State>,
    ready: Condvar,
    space: Condvar,
    clock: Box<dyn Clock>,
}

impl Queue {
    pub(super) fn new(clock: Box<dyn Clock>) -> Self {
        Self {
            state: Mutex::new(State {
                items: VecDeque::new(),
                writes: 0,
                closed: false,
            }),
            ready: Condvar::new(),
            space: Condvar::new(),
            clock,
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn admits(state: &State, priority: Priority) -> bool {
        let limit = match priority {
            Priority::Normal => SHARED_CAPACITY,
            Priority::Warn | Priority::Error => SHARED_CAPACITY + RESERVED_CAPACITY,
        };
        state.writes < limit
    }

    fn enqueue(&self, mut state: MutexGuard<'_, State>, message: Message, counted: bool) {
        state.writes += usize::from(counted);
        state.items.push_back((message, counted));
        drop(state);
        self.ready.notify_one();
    }

    /// Queues a write without waiting, unless it is an error, which waits up to
    /// [`ERROR_WAIT`] for space.
    pub(super) fn push(&self, message: Message) -> Result<(), Rejected> {
        let priority = message.priority();
        if priority == Priority::Error {
            let deadline = self.clock.now() + ERROR_WAIT;
            return self.push_until(message, deadline);
        }
        let state = self.lock();
        if state.closed {
            return Err(Rejected::Closed(message));
        }
        if !Self::admits(&state, priority) {
            return Err(Rejected::Full(message));
        }
        self.enqueue(state, message, true);
        Ok(())
    }

    /// Queues a write, waiting for space until `deadline`.
    pub(super) fn push_until(&self, message: Message, deadline: Instant) -> Result<(), Rejected> {
        let priority = message.priority();
        let mut state = self.lock();
        loop {
            if state.closed {
                return Err(Rejected::Closed(message));
            }
            if Self::admits(&state, priority) {
                self.enqueue(state, message, true);
                return Ok(());
            }
            if self.clock.now() >= deadline {
                return Err(Rejected::Full(message));
            }
            state = self.clock.wait(&self.space, state, deadline);
        }
    }

    /// Queues a control message regardless of capacity; it occupies no write slot.
    pub(super) fn push_control(&self, message: Message) -> Result<(), Message> {
        let state = self.lock();
        if state.closed {
            return Err(message);
        }
        self.enqueue(state, message, false);
        Ok(())
    }

    /// The next message, waiting for one; `None` once closed and drained.
    pub(super) fn pop(&self) -> Option<Message> {
        let mut state = self.lock();
        loop {
            if let Some((message, counted)) = state.items.pop_front() {
                state.writes -= usize::from(counted);
                drop(state);
                if counted {
                    self.space.notify_all();
                }
                return Some(message);
            }
            if state.closed {
                return None;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(|error| error.into_inner());
        }
    }

    /// Rejects further messages; the reader still drains what is queued.
    pub(super) fn close(&self) {
        self.lock().closed = true;
        self.ready.notify_all();
        self.space.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::Stream;
    use std::sync::Arc;

    /// Never blocks: each wait jumps straight to its deadline and records how far it went.
    struct Jumping {
        now: Mutex<Instant>,
        start: Instant,
    }

    impl Clock for Arc<Jumping> {
        fn now(&self) -> Instant {
            *self.now.lock().unwrap()
        }

        fn wait<'a>(
            &self,
            _space: &Condvar,
            state: MutexGuard<'a, State>,
            deadline: Instant,
        ) -> MutexGuard<'a, State> {
            *self.now.lock().unwrap() = deadline;
            state
        }
    }

    fn write(text: &str, priority: Priority) -> Message {
        Message::Write {
            stream: Stream::Stderr,
            priority,
            bytes: text.as_bytes().to_vec(),
        }
    }

    fn queue_with_jumping_clock() -> (Queue, Arc<Jumping>) {
        let start = Instant::now();
        let clock = Arc::new(Jumping {
            now: Mutex::new(start),
            start,
        });
        (Queue::new(Box::new(Arc::clone(&clock))), clock)
    }

    #[test]
    fn normal_writes_cannot_take_the_reserve() {
        let (queue, _clock) = queue_with_jumping_clock();
        let mut accepted = 0;
        for _ in 0..SHARED_CAPACITY * 2 {
            if queue.push(write("info", Priority::Normal)).is_ok() {
                accepted += 1;
            }
        }
        assert_eq!(accepted, SHARED_CAPACITY);
        for _ in 0..RESERVED_CAPACITY {
            assert!(queue.push(write("warn", Priority::Warn)).is_ok());
        }
        assert!(matches!(
            queue.push(write("warn", Priority::Warn)),
            Err(Rejected::Full(_))
        ));
    }

    /// With every slot taken and no reader, an error gives up exactly at its wait bound.
    #[test]
    fn an_error_waits_no_longer_than_its_bound() {
        let (queue, clock) = queue_with_jumping_clock();
        while queue.push(write("warn", Priority::Warn)).is_ok() {}
        let result = queue.push(write("error", Priority::Error));
        assert!(matches!(result, Err(Rejected::Full(_))));
        assert_eq!(*clock.now.lock().unwrap() - clock.start, ERROR_WAIT);
    }

    #[test]
    fn control_messages_bypass_capacity_and_close_drains_in_order() {
        let (queue, _clock) = queue_with_jumping_clock();
        while queue.push(write("info", Priority::Normal)).is_ok() {}
        let (done, _flushed) = std::sync::mpsc::sync_channel(1);
        assert!(queue.push_control(Message::Flush(done)).is_ok());
        queue.close();
        let mut count = 0;
        let mut last_was_flush = false;
        while let Some(message) = queue.pop() {
            count += 1;
            last_was_flush = matches!(message, Message::Flush(_));
        }
        assert_eq!(count, SHARED_CAPACITY + 1);
        assert!(last_was_flush);
        assert!(matches!(
            queue.push(write("late", Priority::Warn)),
            Err(Rejected::Closed(_))
        ));
    }
}

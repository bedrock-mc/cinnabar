//! Bounded sound requests drained by the app audio adapter at its existing frame stage.

mod press;
pub use press::PressSounds;

/// The native interface theme's click definition, resolved through the active pack stack.
pub const UI_CLICK: &str = "random.click";

/// Bounds one frame's queued control sounds.
const MAX_PENDING_UI_SOUNDS: usize = 16;

#[derive(Default)]
struct SoundQueue {
    pending: Vec<(String, f32, f32)>,
}

impl SoundQueue {
    /// Queues a named sound with its authored scalars, up to the frame budget.
    fn push(&mut self, name: &str, volume: f32, pitch: f32) {
        if self.pending.len() < MAX_PENDING_UI_SOUNDS {
            self.pending.push((name.to_owned(), volume, pitch));
        }
    }

    /// Delivers queued sounds in order and retains storage for the next frame.
    fn drain(&mut self, mut receive: impl FnMut(&str, f32, f32)) {
        for (name, volume, pitch) in &self.pending {
            receive(name, *volume, *pitch);
        }
        self.pending.clear();
    }
}

static PENDING_UI_SOUNDS: std::sync::Mutex<SoundQueue> = std::sync::Mutex::new(SoundQueue {
    pending: Vec::new(),
});

/// Requests an interface sound at its declared volume and pitch.
pub fn ui_sound(name: &str, volume: f32, pitch: f32) {
    PENDING_UI_SOUNDS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(name, volume, pitch);
}

/// Delivers pending sounds without allocating on idle frames; the receiver must not requeue.
pub fn drain_sounds(receive: impl FnMut(&str, f32, f32)) {
    PENDING_UI_SOUNDS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .drain(receive);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draining_retains_storage_and_idle_frames_do_not_allocate() {
        let mut queue = SoundQueue::default();
        queue.push(UI_CLICK, 1.0, 1.0);
        let capacity = queue.pending.capacity();
        let mut delivered = 0;
        queue.drain(|name, volume, pitch| {
            assert_eq!((name, volume, pitch), (UI_CLICK, 1.0, 1.0));
            delivered += 1;
        });
        assert_eq!(delivered, 1);
        assert_eq!(queue.pending.capacity(), capacity);
        let (_, allocations) = crate::allocation_count::count(|| {
            for _ in 0..100 {
                queue.drain(|_, _, _| panic!("empty queue"));
            }
        });
        assert_eq!(allocations, 0);
    }

    #[test]
    fn interface_clicks_preserve_authored_volume_and_pitch() {
        let mut queue = SoundQueue::default();
        queue.push(UI_CLICK, 0.75, 1.2);
        queue.push(UI_CLICK, 1.0, 1.0);
        queue.push("ui.reject", 0.5, 1.25);
        let mut sounds = Vec::new();
        queue.drain(|name, volume, pitch| sounds.push((name.to_owned(), volume, pitch)));
        assert_eq!(
            sounds,
            [
                (UI_CLICK.to_owned(), 0.75, 1.2),
                (UI_CLICK.to_owned(), 1.0, 1.0),
                ("ui.reject".to_owned(), 0.5, 1.25)
            ]
        );
    }
}

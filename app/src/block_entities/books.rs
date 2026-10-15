//! Enchanting-table books, ticked at the game rate like the native block actor.

use bevy::platform::collections::HashMap;

use bevy::math::Vec3;
use render_model::{BookPose, EnchantingBook};

/// Most ticks one frame catches up on, so a long hitch does not spin every book.
const MAX_CATCH_UP_TICKS: u64 = 20;

#[derive(Debug, Default)]
pub(super) struct TableBooks {
    /// Each book and the frame that last advanced it.
    books: HashMap<[i32; 3], (EnchantingBook, u64)>,
    frame: u64,
    /// The whole game tick reached by the previous frame.
    tick: Option<u64>,
    steps: u64,
    partial: f32,
}

impl TableBooks {
    /// Starts a frame at `ticks` game ticks; books not advanced before [`Self::finish`] are
    /// forgotten.
    pub(super) fn begin(&mut self, ticks: f64) {
        self.frame = self.frame.wrapping_add(1);
        let ticks = ticks.max(0.0);
        let whole = ticks as u64;
        self.steps = self.tick.map_or(0, |previous| {
            whole.saturating_sub(previous).min(MAX_CATCH_UP_TICKS)
        });
        self.tick = Some(whole);
        self.partial = ticks.fract() as f32;
    }

    /// Runs the book at `position` through this frame's ticks with `players` (eye-level
    /// positions) as possible readers, and returns its pose for drawing.
    pub(super) fn advance(&mut self, position: [i32; 3], players: &[Vec3]) -> BookPose {
        let frame = self.frame;
        // A new book starts with a stamp other than this frame's, so its first draw ticks it.
        let (book, seen) = self
            .books
            .entry(position)
            .or_insert_with(|| (EnchantingBook::new(position), frame.wrapping_add(1)));
        if *seen != frame {
            *seen = frame;
            let reader = book.nearest_reader(players.iter().copied());
            for _ in 0..self.steps {
                book.tick(reader);
            }
        }
        book.pose(self.partial)
    }

    /// Forgets books not advanced this frame; hidden books that advanced remain resident.
    pub(super) fn finish(&mut self) {
        let frame = self.frame;
        self.books.retain(|_, (_, seen)| *seen == frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn books_tick_at_the_game_rate_and_open_for_a_nearby_player() {
        let mut books = TableBooks::default();
        let player = [Vec3::new(0.5, 1.62, 2.0)];
        books.begin(100.0);
        assert_eq!(books.advance([0; 3], &player).open, 0.0, "no ticks yet");
        books.finish();
        books.begin(103.5);
        let pose = books.advance([0; 3], &player);
        assert!(
            (pose.open - 0.25).abs() < 1.0e-5,
            "halfway from the second tick to the third: {}",
            pose.open
        );
        assert_eq!(
            books.advance([0; 3], &player),
            pose,
            "a second draw of the same book does not tick again"
        );
        books.finish();
        books.begin(113.5);
        assert_eq!(
            books.advance([0; 3], &[]).open,
            0.0,
            "closes once they leave"
        );
    }

    #[test]
    fn books_out_of_view_are_forgotten() {
        let mut books = TableBooks::default();
        books.begin(0.0);
        books.advance([0; 3], &[]);
        books.finish();
        books.begin(1.0);
        books.finish();
        assert!(books.books.is_empty());
    }
}

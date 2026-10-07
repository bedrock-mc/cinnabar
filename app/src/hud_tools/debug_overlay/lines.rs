//! Reusable formatting storage for sampled diagnostics.

use std::fmt::{self, Display, Write};

use client_ui::ui_runtime::presentation::DebugLines;

pub(super) const ROW_CAPACITY: usize = 128;

/// Writes rows into retained strings without allocating after their capacity warms.
pub(super) struct Column<'a> {
    lines: Vec<String>,
    next: usize,
    spare: &'a mut Vec<String>,
}

impl<'a> Column<'a> {
    /// Reuses the previous publication's row storage.
    pub(super) fn new(lines: Vec<String>, spare: &'a mut Vec<String>) -> Self {
        Self {
            lines,
            next: 0,
            spare,
        }
    }

    /// Formats one row directly into its reusable string.
    pub(super) fn push(&mut self, value: impl Display) {
        self.push_with(|row| write!(row, "{value}"));
    }

    /// Builds one compound row without intermediate strings.
    pub(super) fn push_with(&mut self, write_row: impl FnOnce(&mut String) -> fmt::Result) {
        if self.next == self.lines.len() {
            self.lines.push(
                self.spare
                    .pop()
                    .unwrap_or_else(|| String::with_capacity(ROW_CAPACITY)),
            );
        }
        let row = &mut self.lines[self.next];
        row.clear();
        write_row(row).expect("writing diagnostics to a String cannot fail");
        self.next += 1;
    }

    /// Formats a fixed sequence into independent retained rows.
    pub(super) fn extend<T: Display>(&mut self, rows: impl IntoIterator<Item = T>) {
        for row in rows {
            self.push(row);
        }
    }

    /// Removes obsolete rows while preserving the vector's capacity.
    pub(super) fn finish(mut self) -> Vec<String> {
        self.spare.extend(self.lines.drain(self.next..));
        self.lines
    }
}

/// Owns a staged pair of columns while the displayed publication remains intact.
pub(super) struct Lines<'a> {
    pub(super) left: Column<'a>,
    pub(super) right: Column<'a>,
}

impl<'a> Lines<'a> {
    /// Reuses storage returned by the previous display publication.
    pub(super) fn new(lines: DebugLines, spare: &'a mut [Vec<String>; 2]) -> Self {
        let [left, right] = spare;
        Self {
            left: Column::new(lines.left, left),
            right: Column::new(lines.right, right),
        }
    }

    /// Returns the completed publication for comparison and swapping.
    pub(super) fn finish(self) -> DebugLines {
        DebugLines {
            left: self.left.finish(),
            right: self.right.finish(),
        }
    }
}

/// Warms both formatting buffers on the first visible publication, preserving row capacity.
pub(super) fn copy_buffers(lines: &DebugLines) -> DebugLines {
    DebugLines {
        left: copy_column(&lines.left),
        right: copy_column(&lines.right),
    }
}

/// Copies one column while reserving the same space as its formatted rows.
fn copy_column(lines: &[String]) -> Vec<String> {
    let mut copied = Vec::with_capacity(lines.len());
    copied.extend(lines.iter().map(|line| {
        let mut copied = String::with_capacity(line.capacity());
        copied.push_str(line);
        copied
    }));
    copied
}

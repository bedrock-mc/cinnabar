//! Work limits shared by summary delivery and cache completion application.

use crate::config::{SUMMARY_BYTES_PER_UPDATE, SUMMARY_COLUMNS_PER_UPDATE};

/// Remaining columns and encoded bytes for one update.
#[derive(Debug)]
pub struct SummaryBudget {
    columns: usize,
    bytes: usize,
    used: bool,
}

impl Default for SummaryBudget {
    fn default() -> Self {
        Self::new(SUMMARY_COLUMNS_PER_UPDATE, SUMMARY_BYTES_PER_UPDATE)
    }
}

impl SummaryBudget {
    /// Limits work to `columns` and `bytes`, allowing one oversized first column.
    #[must_use]
    pub const fn new(columns: usize, bytes: usize) -> Self {
        Self {
            columns,
            bytes,
            used: false,
        }
    }

    /// True while another lookup can be charged, before fetching its encoded length.
    pub(crate) const fn has_columns(&self) -> bool {
        self.columns > 0
    }

    /// Charges one column, or leaves the budget unchanged if it must wait for another frame.
    pub fn take(&mut self, bytes: usize) -> bool {
        if self.columns == 0 || (bytes > self.bytes && self.used) {
            return false;
        }
        self.columns -= 1;
        self.bytes = self.bytes.saturating_sub(bytes);
        self.used = true;
        true
    }
}

//! Admission errors projected at the UI boundary.
use protocol::ChatAutocompleteCatalogError;
use ui::{ChatApplyResult, ChatAutocompleteError, RetainedUiSequenceError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiRuntimeError {
    WrongSession { expected: u64, actual: u64 },
    StaleFifoSequence { previous: u64, actual: u64 },
    StaleBlockCrackSequence { previous: u64, actual: u64 },
    InventoryQueueFull { maximum: usize },
    NonMonotonicLocalTime { previous: u64, actual: u64 },
    NonMonotonicServerTick { previous: u64, actual: u64 },
    TimedEventRequiresLocalClock { fifo_sequence: u64 },
    ChatRejected(ChatApplyResult),
    ChatAutocomplete(ChatAutocompleteError),
    ChatAutocompleteCatalog(ChatAutocompleteCatalogError),
    RetainedUiSequence(RetainedUiSequenceError),
}

impl From<inventory::InventoryIngressError> for UiRuntimeError {
    /// Preserves the app's existing admission error reporting.
    fn from(error: inventory::InventoryIngressError) -> Self {
        match error {
            inventory::InventoryIngressError::WrongSession { expected, actual } => {
                Self::WrongSession { expected, actual }
            }
            inventory::InventoryIngressError::StaleFifoSequence { previous, actual } => {
                Self::StaleFifoSequence { previous, actual }
            }
            inventory::InventoryIngressError::InventoryQueueFull { maximum } => {
                Self::InventoryQueueFull { maximum }
            }
        }
    }
}

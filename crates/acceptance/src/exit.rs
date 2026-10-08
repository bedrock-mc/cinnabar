#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptanceExitDecision {
    Continue,
    WaitForTransparentPresentation,
    Complete,
    Fatal,
    TransparentPresentationTimedOut,
}

impl AcceptanceExitDecision {
    pub const fn is_error(self) -> bool {
        matches!(self, Self::Fatal | Self::TransparentPresentationTimedOut)
    }
}

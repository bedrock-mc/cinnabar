#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AcceptanceExitDecision {
    Continue,
    WaitForTransparentPresentation,
    Complete,
    Fatal,
    TransparentPresentationTimedOut,
}

impl AcceptanceExitDecision {
    pub(crate) const fn is_error(self) -> bool {
        matches!(self, Self::Fatal | Self::TransparentPresentationTimedOut)
    }
}

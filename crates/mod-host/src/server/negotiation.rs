//! The registry confirmation and request for one component instance.

use server_experience::runtime::Command;

pub(super) struct Negotiation {
    pub confirmation: String,
    pub request: Command,
}

impl Negotiation {
    /// Records the confirmation channel and the request to renew when the world changes.
    pub fn new(confirmation: String, request: Command) -> Self {
        Self {
            confirmation,
            request,
        }
    }
}

#[cfg(test)]
mod tests;

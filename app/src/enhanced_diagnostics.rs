//! Explicit admission for bounded, hidden development captures.
use crate::args::ClientArgs;
use anyhow::{Result, ensure};

const DIAGNOSTIC_ENV: &str = "CINNABAR_ENHANCED_DIAGNOSTIC";

/// Rejects unbounded diagnostic launches before native application or GPU creation.
pub(crate) fn configure(args: &ClientArgs) -> Result<()> {
    let Some(value) = std::env::var_os(DIAGNOSTIC_ENV) else {
        return Ok(());
    };
    ensure!(value == "1", "invalid Enhanced diagnostic request");
    ensure!(
        crate::developer_control::hidden_window_requested(),
        "Enhanced diagnostics require a hidden developer client"
    );
    ensure!(
        std::env::var_os(developer_control::ENDPOINT_ENV).is_some(),
        "Enhanced diagnostics require the developer control endpoint"
    );
    ensure!(
        bounded_request(args),
        "Enhanced diagnostics require explicit Enhanced mode, a frame cap up to 30 and a timed exit up to 90 seconds"
    );
    let viewport = std::env::var(developer_control::WINDOW_SIZE_ENV)
        .ok()
        .and_then(|value| developer_control::parse_size(&value))
        .ok_or_else(|| anyhow::anyhow!("Enhanced diagnostics require an explicit viewport"))?;
    render_model::enable_enhanced_diagnostics(viewport).map_err(anyhow::Error::msg)
}

/// Requires a nonzero frame budget and a short explicit diagnostic session.
fn bounded_request(args: &ClientArgs) -> bool {
    args.render_mode == Some(ui::RenderMode::Enhanced)
        && args.frame_cap.is_some_and(|cap| (1..=30).contains(&cap))
        && args
            .acceptance_seconds
            .is_some_and(|seconds| (1..=90).contains(&seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_require_explicit_mode_and_nonzero_bounded_budgets() {
        let mut args = ClientArgs::default();
        assert!(!bounded_request(&args));
        args.render_mode = Some(ui::RenderMode::Enhanced);
        args.frame_cap = Some(30);
        args.acceptance_seconds = Some(90);
        assert!(bounded_request(&args));
        for cap in [None, Some(0), Some(31)] {
            args.frame_cap = cap;
            assert!(!bounded_request(&args));
        }
        args.frame_cap = Some(1);
        for seconds in [None, Some(0), Some(91)] {
            args.acceptance_seconds = seconds;
            assert!(!bounded_request(&args));
        }
    }
}

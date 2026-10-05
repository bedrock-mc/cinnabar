use crate::error::JolyneError;
use std::future::Future;
use std::time::Instant;

pub(super) async fn measure<T>(
    stage: &'static str,
    work: impl Future<Output = Result<T, JolyneError>>,
) -> Result<T, JolyneError> {
    let started = Instant::now();
    let result = work.await;
    tracing::info!(
        stage,
        succeeded = result.is_ok(),
        elapsed_ms = started.elapsed().as_secs_f64() * 1e3,
        "Bedrock login stage complete"
    );
    result
}

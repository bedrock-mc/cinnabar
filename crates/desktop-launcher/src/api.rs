//! Read-only requests to the Cinnabar API origin the core and packaging share.

use anyhow::{Result, ensure};
use std::time::Duration;

const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BODY_BYTES: usize = 256 * 1024;

/// The URL of `route` on the API origin.
pub fn url(route: &str) -> String {
    format!("{}{route}", bridge::contract::CINNABAR_API_BASE_URL)
}

/// Fetches `route` over HTTPS with a bounded body.
pub async fn get(route: &str) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .https_only(true)
        .timeout(FETCH_TIMEOUT)
        .build()?;
    let mut response = client.get(url(route)).send().await?;
    ensure!(
        response.status() == reqwest::StatusCode::OK,
        "{route} request failed: {}",
        response.status()
    );
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            body.len() + chunk.len() <= MAX_BODY_BYTES,
            "{route} exceeds size limit"
        );
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    #[test]
    fn routes_live_on_the_shared_https_origin() {
        assert!(super::url("/v1/news").starts_with("https://"));
        assert!(super::url("/v1/news").ends_with("/v1/news"));
    }
}

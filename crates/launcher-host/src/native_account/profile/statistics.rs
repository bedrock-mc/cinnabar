//! Retail aggregation, preserving unavailable values separately from real zero.

use super::policy;
use bedrock_services::xbox::statistics::Batch;

/// Uses the first requested user and finite source-order double sums, as the Go fallback does.
pub(super) fn project(batch: &Batch, xuid: &str) -> Option<bridge::ProfileStatistics> {
    let user = batch.users.iter().find(|user| user.xuid == xuid)?;
    let policy = &policy::get().statistics;
    let mut totals = [0.0_f64; 4];
    let mut available = [false; 4];
    for group in &user.configurations {
        if !policy.configuration_ids.contains(&group.service_config_id) {
            continue;
        }
        for statistic in &group.statistics {
            let Some(index) = policy.names.iter().position(|name| name == &statistic.name) else {
                continue;
            };
            let Some(number) = statistic
                .non_negative_number()
                .and_then(|text| text.parse::<f64>().ok())
            else {
                continue;
            };
            let total = totals[index] + number;
            if total.is_finite() {
                totals[index] = total;
                available[index] = true;
            }
        }
    }
    let [
        minutes_played,
        blocks_broken,
        mobs_defeated,
        distance_travelled,
    ] = std::array::from_fn(|index| available[index].then(|| totals[index].to_string()));
    Some(bridge::ProfileStatistics {
        minutes_played,
        blocks_broken,
        mobs_defeated,
        distance_travelled,
    })
}

//! Application retail selection, checked against the actual Go request owner.

use bedrock_services::xbox::statistics::Selection;
use serde::Deserialize;
use std::sync::LazyLock;
use uuid::Uuid;

#[derive(Deserialize)]
pub(super) struct Policy {
    schema: u32,
    pub(super) statistics: Statistics,
    pub(super) achievement_title: u32,
}

#[derive(Deserialize)]
pub(super) struct Statistics {
    pub(super) configuration_ids: [Uuid; 7],
    pub(super) names: [String; 4],
}

/// Borrows the single checked policy used by actual native profile requests.
pub(super) fn get() -> &'static Policy {
    static POLICY: LazyLock<Policy> = LazyLock::new(|| {
        let policy: Policy = serde_json::from_str(include_str!(
            "../../../../../core/catalog/native_profile_policy.json"
        ))
        .expect("checked native profile policy");
        assert_eq!(policy.schema, 1, "supported native profile policy");
        policy
    });
    &POLICY
}

/// Derives service selections without maintaining another platform or statistic table.
pub(super) fn statistics() -> Vec<Selection> {
    let policy = &get().statistics;
    policy
        .configuration_ids
        .iter()
        .map(|id| Selection {
            service_config_id: *id,
            names: policy.names.to_vec(),
        })
        .collect()
}

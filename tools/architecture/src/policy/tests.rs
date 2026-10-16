use super::Policy;

const REQUIRED: &str =
    "production_rust_max=1000\nmodule_root_max=300\npowershell_max=800\ntest_max=1200\n";

#[test]
fn policy_rejects_unknown_restrictions() {
    for extra in [
        "forbiden_artifacts=['secret']\n",
        "[[crates]]\nname='sample'\npath='.'\nforbiden_dependencies=['bevy']\n",
        "[[module_boundaries]]\npath='src'\nforbiden_owned_types=['Owner']\n",
        "[timing]\nclock_free_crate=['sample']\n",
    ] {
        assert!(toml::from_str::<Policy>(&format!("{REQUIRED}{extra}")).is_err());
    }
}

#[test]
fn known_optional_rules_keep_their_defaults() {
    let policy: Policy =
        toml::from_str(&format!("{REQUIRED}[[crates]]\nname='sample'\npath='.'\n")).unwrap();
    assert!(policy.module_boundaries.is_empty());
    assert!(policy.forbidden_artifacts.is_empty());
    assert!(policy.timing.clock_free_crates.is_empty());
    assert!(policy.crate_rules[0].forbidden_dependencies.is_empty());
    assert!(!policy.crate_rules[0].dependency_free);
}

#[test]
fn repository_policy_uses_only_known_fields() {
    toml::from_str::<Policy>(include_str!("../../policy.toml")).unwrap();
}

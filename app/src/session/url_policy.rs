//! Native archive scheduling retained across the application's join snapshots.

use super::{MenuRuntime, SessionController};

impl SessionController {
    /// Retains the explicit launch choice through menu joins, retries and server transfers.
    pub(crate) fn with_parallel_resource_pack_urls(mut self, enabled: bool) -> Self {
        self.parallel_resource_pack_urls = enabled;
        self
    }

    /// Captures player preferences with this controller's original local acquisition choice.
    pub(super) fn login_settings(
        &self,
        menu: &MenuRuntime,
        input: Option<&client_state::SemanticInputSnapshot>,
    ) -> protocol::LoginSettings {
        super::login_settings(menu, input, self.parallel_resource_pack_urls)
    }
}

#[cfg(all(test, feature = "native-networking"))]
mod tests {
    use super::*;
    use crate::args::{ClientArgs, ParseOutcome};

    #[test]
    fn native_url_policy_reaches_direct_and_retained_controller_snapshots() {
        let layout = launcher::test_support::scratch("native-pack-url-policy");
        let menu = MenuRuntime::new_with_layout(
            false,
            None,
            "Fixture".into(),
            layout,
            client_shell::player_skin::LocalPlayerSkin::generated_default("Fixture"),
        );
        for parallel in [false, true] {
            for direct in [false, true] {
                let mut arguments = vec!["client", "--native-networking", "raknet"];
                if direct {
                    arguments.extend(["--address", "127.0.0.1:19132"]);
                }
                if parallel {
                    arguments.push("--parallel-resource-pack-urls");
                }
                let ParseOutcome::Run(args) = ClientArgs::parse_from(arguments).unwrap() else {
                    panic!("native launch settings")
                };
                let mut controller = SessionController::default()
                    .with_native_transport(args.native_transport)
                    .with_parallel_resource_pack_urls(args.parallel_resource_pack_urls);
                let direct_settings =
                    super::super::login_settings(&menu, None, args.parallel_resource_pack_urls);
                assert_eq!(direct_settings.parallel_resource_pack_urls, parallel);
                assert_eq!(controller.login_settings(&menu, None), direct_settings);
                controller.begin_fresh_transfer_chain();
                assert!(controller.consume_transfer_chain_hop());
                controller.next_generation();
                assert_eq!(controller.login_settings(&menu, None), direct_settings);
            }
        }
    }
}

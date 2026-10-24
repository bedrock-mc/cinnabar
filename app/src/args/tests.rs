use super::{ArgsError, ClientArgs, HELP, ParseOutcome};
use std::path::PathBuf;

/// A native build selects addressed discovery; a build without it cannot create a native endpoint.
#[test]
fn native_default_selects_addressed_without_a_process_backend() {
    let expected = cfg!(all(
        feature = "native-networking",
        not(target_arch = "wasm32")
    ))
    .then_some(protocol::NativeTransport::Addressed);
    let ParseOutcome::Run(menu) = ClientArgs::parse_from(["client"]).unwrap() else {
        panic!("menu arguments");
    };
    assert_eq!(ClientArgs::default().native_transport, expected);
    assert_eq!(menu.native_transport, expected);
    assert!(!menu.connection_requested());
    let ParseOutcome::Run(direct) =
        ClientArgs::parse_from(["client", "--address", "fixture.invalid"]).unwrap()
    else {
        panic!("direct arguments");
    };
    assert_eq!(direct.native_transport, expected);
    if expected.is_some() {
        let protocol::SessionEndpoint::Native(target) = direct.session_endpoint().unwrap();
        assert_eq!(target.transport, protocol::NativeTransport::Addressed);
        assert_eq!(target.address, "fixture.invalid");
    } else {
        assert!(matches!(
            direct.session_endpoint(),
            Err(ArgsError::NativeNetworkingUnavailable)
        ));
    }
}

#[test]
fn removed_process_backend_options_are_rejected() {
    for option in ["--legacy-networking", "--socket-dir"] {
        assert!(
            matches!(ClientArgs::parse_from(["client", option]), Err(ArgsError::Unknown(value)) if value == option)
        );
    }
}

#[cfg(all(
    feature = "native-networking",
    feature = "acceptance",
    not(target_arch = "wasm32")
))]
#[test]
fn native_timed_sessions_require_a_destination() {
    assert!(matches!(
        ClientArgs::parse_from(["client", "--acceptance-seconds", "1"]),
        Err(ArgsError::NativeNetworkingUnavailable)
    ));
}

#[test]
fn parallel_pack_urls_require_the_selected_native_backend() {
    assert!(!ClientArgs::default().parallel_resource_pack_urls);
    let selected = ClientArgs::parse_from(["client", "--parallel-resource-pack-urls"]);
    if cfg!(all(
        feature = "native-networking",
        not(target_arch = "wasm32")
    )) {
        let ParseOutcome::Run(args) = selected.unwrap() else {
            panic!("native URL policy arguments");
        };
        assert!(args.parallel_resource_pack_urls);
        assert_eq!(
            args.native_transport,
            Some(protocol::NativeTransport::Addressed)
        );
    } else {
        assert!(matches!(
            selected,
            Err(ArgsError::ParallelPackUrlsRequireNativeNetworking)
        ));
    }
    if !cfg!(all(
        feature = "native-networking",
        not(target_arch = "wasm32")
    )) {
        assert!(matches!(
            ClientArgs::parse_from([
                "client",
                "--native-networking",
                "raknet",
                "--parallel-resource-pack-urls",
            ]),
            Err(ArgsError::NativeNetworkingUnavailable)
        ));
    }
}

#[test]
fn native_networking_allows_menu_selection_and_rejects_ambiguous_endpoints() {
    let menu = ClientArgs::parse_from(["client", "--native-networking", "raknet"]);
    if cfg!(all(
        feature = "native-networking",
        not(target_arch = "wasm32")
    )) {
        let ParseOutcome::Run(menu) = menu.unwrap() else {
            panic!("menu launch")
        };
        assert!(!menu.connection_requested());
        assert_eq!(
            menu.native_transport,
            Some(protocol::NativeTransport::RakNet)
        );
    } else {
        assert!(matches!(menu, Err(ArgsError::NativeNetworkingUnavailable)));
    }
    assert!(matches!(
        ClientArgs::parse_from([
            "client",
            "--native-networking",
            "raknet",
            "--acceptance-seconds",
            "1"
        ]),
        Err(ArgsError::NativeNetworkingUnavailable)
    ));
    assert!(matches!(
        ClientArgs::parse_from(["client", "--native-networking", "unknown"]),
        Err(ArgsError::InvalidNativeTransport)
    ));
}

/// Explicit native CLI selection cannot turn a display-only friend address into a Go route.
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
#[test]
fn explicit_native_friend_startup_never_selects_core_from_a_display_target() {
    let address = format!("{}2535", bridge::contract::FRIEND_TARGET_PREFIX);
    let mut outcomes = Vec::new();
    for mode in ["addressed", "raknet", "nethernet-lan"] {
        let ParseOutcome::Run(native) = ClientArgs::parse_from([
            "client",
            "--address",
            address.as_str(),
            "--native-networking",
            mode,
        ])
        .unwrap() else {
            panic!("explicit native fixture arguments");
        };
        outcomes.push(native.session_endpoint());
    }
    assert!(
        outcomes.iter().all(Result::is_err),
        "explicit native friend startup silently selected a Core endpoint"
    );
}

#[test]
fn accepts_file_open_arguments_and_explicit_imports() {
    let ParseOutcome::Run(args) = ClientArgs::parse_from([
        "client",
        "a.mcpack",
        "b.MCADDON",
        "c.zip",
        "--import-pack",
        "d.mcpack",
    ])
    .unwrap() else {
        panic!("run");
    };
    assert_eq!(
        args.import_packs,
        ["a.mcpack", "b.MCADDON", "c.zip", "d.mcpack"].map(PathBuf::from)
    );
    assert!(!args.connection_requested());
}

#[test]
fn defaults_are_stable() {
    let ParseOutcome::Run(args) = ClientArgs::parse_from(["client"]).unwrap() else {
        panic!("expected run args")
    };
    assert_eq!(args.assets, None);
    assert_eq!(args.display_name, launcher::PRODUCT_NAME);
    assert_eq!(args.acceptance_seconds, None);
    assert_eq!(args.metrics_warmup_seconds, 0);
    assert_eq!(args.metrics_sample_seconds, None);
    assert!(!args.auto_fly);
    assert!(!args.force_vsync);
    assert!(!args.no_vsync);
    assert_eq!(args.frame_cap, None);
    assert!(!args.full_view_teleport_gate);
    assert!(!args.require_transparent_presentation);
    assert_eq!(args.transparent_witness_request, None);
    assert_eq!(args.model_witness_request, None);
    assert_eq!(args.movement_evidence_target, None);
    assert!(!args.movement_candidate_physics);
}

#[test]
fn parses_every_acceptance_flag() {
    let ParseOutcome::Run(args) = ClientArgs::parse_from([
        "client",
        "--address",
        "fixture.invalid:19132",
        "--assets",
        "assets/vanilla.mcbea",
        "--display-name",
        "TestBot",
        "--acceptance-seconds",
        "900",
        "--metrics-out",
        "metrics.json",
        "--metrics-warmup-seconds",
        "30",
        "--metrics-sample-seconds",
        "120",
        "--auto-fly",
        "--no-vsync",
        "--require-transparent-presentation",
        "--transparent-witness-request",
        "run/transparent-witness-request.json",
        "--model-witness-request",
        "run/model-witness-request.json",
        "--movement-evidence-target",
        "Zeno",
        "--movement-candidate-physics",
    ])
    .unwrap() else {
        panic!("expected run args")
    };
    assert_eq!(args.assets, Some(PathBuf::from("assets/vanilla.mcbea")));
    assert_eq!(args.display_name, "TestBot");
    assert_eq!(args.acceptance_seconds, Some(900));
    assert_eq!(args.metrics_out, Some(PathBuf::from("metrics.json")));
    assert_eq!(args.metrics_warmup_seconds, 30);
    assert_eq!(args.metrics_sample_seconds, Some(120));
    assert!(args.auto_fly);
    assert!(args.no_vsync);
    assert_eq!(args.frame_cap, None);
    assert!(!args.full_view_teleport_gate);
    assert!(args.require_transparent_presentation);
    assert_eq!(
        args.transparent_witness_request,
        Some(PathBuf::from("run/transparent-witness-request.json"))
    );
    assert_eq!(
        args.model_witness_request,
        Some(PathBuf::from("run/model-witness-request.json"))
    );
    assert_eq!(
        args.movement_evidence_target,
        Some(super::EvidenceTarget::Zeno)
    );
    assert!(args.movement_candidate_physics);
}

#[test]
fn parses_full_view_teleport_gate_and_frame_cap() {
    let ParseOutcome::Run(args) =
        ClientArgs::parse_from(["client", "--full-view-teleport-gate", "--frame-cap", "60"])
            .unwrap()
    else {
        panic!("expected run args")
    };

    assert!(args.full_view_teleport_gate);
    assert_eq!(args.frame_cap, Some(60));
}

#[test]
fn parses_model_witness_request_path() {
    let ParseOutcome::Run(args) = ClientArgs::parse_from([
        "client",
        "--model-witness-request",
        "run/model-witness-request.json",
    ])
    .unwrap() else {
        panic!("expected run args")
    };
    assert_eq!(
        args.model_witness_request,
        Some(PathBuf::from("run/model-witness-request.json"))
    );
}

#[test]
fn parses_explicit_vsync_override() {
    let ParseOutcome::Run(args) = ClientArgs::parse_from(["client", "--vsync"]).unwrap() else {
        panic!("expected run args")
    };
    assert!(args.force_vsync);
    assert!(!args.no_vsync);
}

#[test]
fn help_documents_all_acceptance_flags() {
    assert_eq!(
        ClientArgs::parse_from(["client", "--help"]).unwrap(),
        ParseOutcome::Help
    );
    for flag in [
        "--address",
        "--assets",
        "--acceptance-seconds",
        "--metrics-out",
        "--metrics-warmup-seconds",
        "--metrics-sample-seconds",
        "--auto-fly",
        "--vsync",
        "--no-vsync",
        "--frame-cap",
        "--full-view-teleport-gate",
        "--require-transparent-presentation",
        "--transparent-witness-request",
        "--model-witness-request",
        "--movement-evidence-target",
        "--movement-candidate-physics",
        "--dev-debug-overlay",
    ] {
        assert!(HELP.contains(flag));
    }
}

#[test]
fn render_mode_is_an_optional_validated_session_override() {
    let ParseOutcome::Run(args) = ClientArgs::parse_from(["client"]).unwrap() else {
        panic!("expected run");
    };
    assert_eq!(args.render_mode, None);
    let ParseOutcome::Run(args) =
        ClientArgs::parse_from(["client", "--render-mode", "Enhanced"]).unwrap()
    else {
        panic!("expected run");
    };
    assert_eq!(args.render_mode, Some(ui::RenderMode::Enhanced));
    assert_eq!(
        ClientArgs::parse_from(["client", "--render-mode", "ultra"]),
        Err(ArgsError::InvalidRenderMode("ultra".to_owned()))
    );
}

#[test]
fn hide_toasts_is_a_developer_only_flag() {
    let args = ClientArgs {
        hide_toasts: true,
        ..ClientArgs::default()
    };
    assert_eq!(args.validate_developer_support(true), Ok(()));
    assert_eq!(
        args.validate_developer_support(false),
        Err(ArgsError::DeveloperControlFeatureRequired("--hide-toasts"))
    );
    assert_eq!(
        ClientArgs::default().validate_developer_support(false),
        Ok(())
    );
    let parsed = ClientArgs::parse_from(["client", "--hide-toasts"]);
    if cfg!(feature = "developer-control") {
        let Ok(ParseOutcome::Run(args)) = parsed else {
            panic!("expected run args, got {parsed:?}");
        };
        assert!(args.hide_toasts);
    } else {
        assert_eq!(
            parsed,
            Err(ArgsError::DeveloperControlFeatureRequired("--hide-toasts"))
        );
    }
    assert!(HELP.contains("--hide-toasts"));
}

#[test]
fn fixed_time_is_a_validated_developer_only_pin() {
    let args = ClientArgs {
        fixed_time: Some(6_000),
        ..ClientArgs::default()
    };
    assert_eq!(args.validate_developer_support(true), Ok(()));
    assert_eq!(
        args.validate_developer_support(false),
        Err(ArgsError::DeveloperControlFeatureRequired("--fixed-time"))
    );
    let parsed = ClientArgs::parse_from(["client", "--fixed-time", "13013"]);
    if cfg!(feature = "developer-control") {
        let Ok(ParseOutcome::Run(args)) = parsed else {
            panic!("expected run args, got {parsed:?}");
        };
        assert_eq!(args.fixed_time, Some(13_013));
    } else {
        assert_eq!(
            parsed,
            Err(ArgsError::DeveloperControlFeatureRequired("--fixed-time"))
        );
    }
    assert_eq!(
        ClientArgs::parse_from(["client", "--fixed-time", "-1"]),
        Err(ArgsError::InvalidFixedTime("-1".to_owned()))
    );
    assert!(HELP.contains("--fixed-time <TICKS>"));
}

#[test]
fn malformed_arguments_are_rejected() {
    assert!(matches!(
        ClientArgs::parse_from(["client", "--address"]),
        Err(ArgsError::MissingValue { flag: "--address" })
    ));
    assert!(matches!(
        ClientArgs::parse_from(["client", "--vsync", "--no-vsync"]),
        Err(ArgsError::ConflictingVsyncFlags)
    ));
    assert!(matches!(
        ClientArgs::parse_from(["client", "--acceptance-seconds", "0"]),
        Err(ArgsError::InvalidAcceptanceSeconds(_))
    ));
    assert!(matches!(
        ClientArgs::parse_from(["client", "--frame-cap", "0"]),
        Err(ArgsError::InvalidFrameCap(_))
    ));
    assert!(matches!(
        ClientArgs::parse_from(["client", "--gui-scale", "5"]),
        Err(ArgsError::InvalidGuiScale(_))
    ));
    let ParseOutcome::Run(parsed) = ClientArgs::parse_from(["client", "--gui-scale", "3"]).unwrap()
    else {
        panic!("--gui-scale must parse into a run outcome");
    };
    assert_eq!(parsed.gui_scale, Some(3));
    assert!(parsed.dev_debug_overlay);
    let ParseOutcome::Run(parsed) =
        ClientArgs::parse_from(["client", "--dev-debug-overlay"]).unwrap()
    else {
        panic!("--dev-debug-overlay must parse into a run outcome");
    };
    assert!(parsed.dev_debug_overlay);
    assert!(matches!(
        ClientArgs::parse_from(["client", "--unknown"]),
        Err(ArgsError::Unknown(_))
    ));
    assert!(matches!(
        ClientArgs::parse_from([
            "client",
            "--acceptance-seconds",
            "30",
            "--movement-candidate-physics"
        ]),
        Err(ArgsError::MovementCandidateRequiresEvidence)
    ));
    assert!(matches!(
        ClientArgs::parse_from([
            "client",
            "--acceptance-seconds",
            "30",
            "--movement-evidence-target",
            "Lunar",
            "--movement-candidate-physics"
        ]),
        Err(ArgsError::MovementEvidenceRequiresAttributableRun)
    ));
    assert!(matches!(
        ClientArgs::parse_from(["client", "--movement-evidence-target", "Unknown"]),
        Err(ArgsError::InvalidEvidenceTarget(_))
    ));
}

#[test]
fn shader_pack_is_a_validated_session_override_of_the_shaderpack_mode() {
    let ParseOutcome::Run(args) =
        ClientArgs::parse_from(["client", "--render-mode", "ShaderPack"]).unwrap()
    else {
        panic!("expected run");
    };
    assert_eq!(args.render_mode, Some(ui::RenderMode::ShaderPack));
    assert_eq!(args.shader_pack, None);
    for arguments in [
        &["client", "--shader-pack", "packs/Dusk.zip"][..],
        &[
            "client",
            "--render-mode",
            "shaderpack",
            "--shader-pack",
            "packs/Dusk.zip",
        ][..],
    ] {
        let ParseOutcome::Run(args) = ClientArgs::parse_from(arguments).unwrap() else {
            panic!("expected run");
        };
        assert_eq!(args.render_mode, Some(ui::RenderMode::ShaderPack));
        assert_eq!(args.shader_pack, Some(PathBuf::from("packs/Dusk.zip")));
    }
    assert_eq!(
        ClientArgs::parse_from([
            "client",
            "--shader-pack",
            "Dusk",
            "--render-mode",
            "enhanced"
        ]),
        Err(ArgsError::ConflictingShaderPackRenderMode("enhanced"))
    );
    assert_eq!(
        ClientArgs::parse_from(["client", "--shader-pack", ""]),
        Err(ArgsError::EmptyShaderPack)
    );
    assert_eq!(
        ClientArgs::parse_from(["client", "--shader-pack"]),
        Err(ArgsError::MissingValue {
            flag: "--shader-pack"
        })
    );
    assert!(HELP.contains("--shader-pack <PATH>"));
    assert!(HELP.contains("shaderpack"));
}

#[test]
fn frame_time_counter_is_a_validated_developer_only_pin() {
    let args = ClientArgs {
        frame_time_counter_ms: Some(12_500),
        ..ClientArgs::default()
    };
    assert_eq!(args.validate_developer_support(true), Ok(()));
    assert_eq!(
        args.validate_developer_support(false),
        Err(ArgsError::DeveloperControlFeatureRequired(
            "--frame-time-counter"
        ))
    );
    let parsed = ClientArgs::parse_from(["client", "--frame-time-counter", "12.5"]);
    if cfg!(feature = "developer-control") {
        let Ok(ParseOutcome::Run(args)) = parsed else {
            panic!("expected run args, got {parsed:?}");
        };
        assert_eq!(args.frame_time_counter_ms, Some(12_500));
    } else {
        assert_eq!(
            parsed,
            Err(ArgsError::DeveloperControlFeatureRequired(
                "--frame-time-counter"
            ))
        );
    }
    for bad in ["-1", "3600", "NaN", "soon"] {
        assert_eq!(
            ClientArgs::parse_from(["client", "--frame-time-counter", bad]),
            Err(ArgsError::InvalidFrameTimeCounter(bad.to_owned()))
        );
    }
    assert!(HELP.contains("--frame-time-counter <SECONDS>"));
}

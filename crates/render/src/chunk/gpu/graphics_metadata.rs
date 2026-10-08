//! Graphics adapter and present-mode metadata for diagnostic runs, from the shared surface probe.

use crate::chunk::*;
use crate::present_mode::{
    PresentModePolicy, PresentModePreference, PresentModeRemedy, requested_present_mode_kind,
    resolve_dx12_present_mode_remedy,
};
use crate::surface_capabilities::ProbedSurface;
use render_model::{PresentModeKind, SurfacePresentModes, configured_present_mode};

pub(in crate::chunk) fn adapter_metadata_field(value: String) -> String {
    if value.trim().is_empty() {
        "unavailable".to_owned()
    } else {
        value
    }
}

#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) enum GraphicsMetadataPublicationState {
    #[default]
    Pending,
    AwaitingAutomaticImmediate {
        window: u64,
    },
    Published,
}

impl GraphicsMetadataPublicationState {
    fn should_probe(
        &mut self,
        window: u64,
        preference: Option<PresentModePreference>,
        requested: bevy::window::PresentMode,
    ) -> bool {
        match *self {
            Self::Published => false,
            Self::AwaitingAutomaticImmediate {
                window: pending_window,
            } if pending_window == window
                && preference == Some(PresentModePreference::Auto)
                && requested == bevy::window::PresentMode::Fifo =>
            {
                false
            }
            Self::AwaitingAutomaticImmediate { .. } => {
                *self = Self::Pending;
                true
            }
            Self::Pending => true,
        }
    }

    fn await_automatic_immediate(&mut self, window: u64) {
        *self = Self::AwaitingAutomaticImmediate { window };
    }

    fn publish(&mut self) {
        *self = Self::Published;
    }
}

fn metadata_requires_automatic_immediate(
    preference: Option<PresentModePreference>,
    backend: wgpu::Backend,
    adapter: &str,
    driver: &str,
    requested: bevy::window::PresentMode,
    supported: SurfacePresentModes,
) -> bool {
    preference.is_some_and(|preference| {
        resolve_dx12_present_mode_remedy(preference, backend, adapter, driver, requested, supported)
            == PresentModeRemedy::UseImmediate
    })
}

/// Without a policy the window's request is final. With one, only the mode the main world
/// selected from probed capabilities counts, never the fallback requested before the probe.
fn selection_applied(
    selection: Option<Option<PresentModeKind>>,
    requested: PresentModeKind,
) -> bool {
    selection.is_none_or(|selected| selected == Some(requested))
}

/// Publication is eligible only until the requested diagnostic metadata is published.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::chunk) struct GraphicsMetadataPublication;

/// Orders publication after the surface probe and remedy, before surface configuration.
pub(in crate::chunk) fn configure_graphics_metadata_publication(schedule: &mut Schedule) {
    schedule.configure_sets(
        GraphicsMetadataPublication
            .run_if(graphics_metadata_pending)
            .after(RenderSystems::ExtractCommands)
            .after(crate::surface_capabilities::SurfaceCapabilitiesSet)
            .after(crate::present_mode::PresentModePolicySet)
            .before(bevy::render::view::window::create_surfaces),
    );
}

/// Skips the system entirely once nothing remains to publish.
fn graphics_metadata_pending(
    input: Res<VisibilityDiagnosticsInput>,
    publication: Res<GraphicsMetadataPublicationState>,
) -> bool {
    input.enabled() && *publication != GraphicsMetadataPublicationState::Published
}

#[derive(SystemParam)]
pub(in crate::chunk) struct GraphicsRuntimeMetadataInputs<'w> {
    windows: Res<'w, ExtractedWindows>,
    probed: Res<'w, ProbedSurface>,
    render_adapter: Res<'w, RenderAdapter>,
    policy: Option<Res<'w, PresentModePolicy>>,
    input: Res<'w, VisibilityDiagnosticsInput>,
    diagnostics: Res<'w, VisibilityDiagnostics>,
}

pub(in crate::chunk) fn publish_graphics_runtime_metadata(
    inputs: GraphicsRuntimeMetadataInputs,
    mut publication: ResMut<GraphicsMetadataPublicationState>,
) {
    let GraphicsRuntimeMetadataInputs {
        windows,
        probed,
        render_adapter,
        policy,
        input,
        diagnostics,
    } = inputs;
    if !input.enabled() || *publication == GraphicsMetadataPublicationState::Published {
        return;
    }
    let Some(window_id) = windows.primary else {
        return;
    };
    let Some(window) = windows.windows.get(&window_id) else {
        return;
    };
    let preference = policy.as_deref().map(PresentModePolicy::preference);
    if !publication.should_probe(window_id.to_bits(), preference, window.present_mode) {
        return;
    }
    let Some(requested) = requested_present_mode_kind(window.present_mode) else {
        return;
    };
    let Some(supported) = probed.modes_for(window_id) else {
        return;
    };
    let adapter_info = render_adapter.get_info();
    if metadata_requires_automatic_immediate(
        preference,
        adapter_info.backend,
        &adapter_info.name,
        &adapter_info.driver,
        window.present_mode,
        supported,
    ) {
        publication.await_automatic_immediate(window_id.to_bits());
        return;
    }
    if !selection_applied(
        policy.as_deref().map(PresentModePolicy::selection),
        requested,
    ) {
        return;
    }
    diagnostics.publish_graphics_adapter(GraphicsAdapterMetadata {
        backend: format!("{:?}", adapter_info.backend),
        adapter: adapter_metadata_field(adapter_info.name),
        driver: adapter_metadata_field(adapter_info.driver),
        driver_info: adapter_metadata_field(adapter_info.driver_info),
        requested_present_mode: requested.name().to_owned(),
        effective_present_mode: configured_present_mode(requested, supported)
            .name()
            .to_owned(),
        present_mode_proven: true,
    });
    publication.publish();
}

#[cfg(test)]
mod graphics_metadata_tests {
    use super::*;

    #[derive(Resource, Default)]
    struct NativeProbeCalls(usize);

    /// Counts native dispatches, including no-op calls that still require the main thread.
    fn record_native_probe(
        _main_thread: bevy::ecs::system::NonSendMarker,
        mut calls: ResMut<NativeProbeCalls>,
    ) {
        calls.0 += 1;
    }

    #[test]
    fn inactive_graphics_metadata_never_dispatches_native_work() {
        let mut world = World::new();
        world.insert_resource(VisibilityDiagnosticsInput::new(false));
        world.init_resource::<GraphicsMetadataPublicationState>();
        world.init_resource::<NativeProbeCalls>();
        let mut schedule = Schedule::default();
        configure_graphics_metadata_publication(&mut schedule);
        schedule.add_systems(record_native_probe.in_set(GraphicsMetadataPublication));
        schedule.run(&mut world);
        assert_eq!(world.resource::<NativeProbeCalls>().0, 0);

        world.insert_resource(VisibilityDiagnosticsInput::new(true));
        schedule.run(&mut world);
        assert_eq!(world.resource::<NativeProbeCalls>().0, 1);

        world
            .resource_mut::<GraphicsMetadataPublicationState>()
            .publish();
        for _ in 0..3 {
            schedule.run(&mut world);
        }
        assert_eq!(world.resource::<NativeProbeCalls>().0, 1);
    }

    #[test]
    fn startup_graphics_metadata_dispatches_until_publication_succeeds() {
        let mut world = World::new();
        let mut input = VisibilityDiagnosticsInput::new(false);
        input.set_startup_probe_enabled(true);
        world.insert_resource(input);
        world.init_resource::<GraphicsMetadataPublicationState>();
        world.init_resource::<NativeProbeCalls>();
        let mut schedule = Schedule::default();
        configure_graphics_metadata_publication(&mut schedule);
        schedule.add_systems(record_native_probe.in_set(GraphicsMetadataPublication));
        for _ in 0..2 {
            schedule.run(&mut world);
        }
        assert_eq!(world.resource::<NativeProbeCalls>().0, 2);
        world
            .resource_mut::<GraphicsMetadataPublicationState>()
            .publish();
        schedule.run(&mut world);
        assert_eq!(world.resource::<NativeProbeCalls>().0, 2);
    }

    const AFFECTED_ADAPTER: &str = "Radeon RX 570 Series";
    const AFFECTED_DRIVER: &str = "31.0.21924.61";

    #[test]
    fn exact_auto_fifo_metadata_waits_for_automatic_immediate() {
        let supported = SurfacePresentModes::FIFO_ONLY.with(PresentModeKind::Immediate);
        assert!(metadata_requires_automatic_immediate(
            Some(PresentModePreference::Auto),
            wgpu::Backend::Dx12,
            AFFECTED_ADAPTER,
            AFFECTED_DRIVER,
            bevy::window::PresentMode::Fifo,
            supported,
        ));
        for (preference, requested, adapter) in [
            (
                Some(PresentModePreference::Vsync),
                bevy::window::PresentMode::Fifo,
                AFFECTED_ADAPTER,
            ),
            (
                Some(PresentModePreference::NoVsync),
                bevy::window::PresentMode::Immediate,
                AFFECTED_ADAPTER,
            ),
            (
                Some(PresentModePreference::Auto),
                bevy::window::PresentMode::Fifo,
                "Radeon RX 580 Series",
            ),
            (None, bevy::window::PresentMode::Fifo, AFFECTED_ADAPTER),
        ] {
            assert!(!metadata_requires_automatic_immediate(
                preference,
                wgpu::Backend::Dx12,
                adapter,
                AFFECTED_DRIVER,
                requested,
                supported,
            ));
        }
    }

    #[test]
    fn deferred_metadata_does_not_reprobe_fifo_and_releases_on_immediate() {
        let mut state = GraphicsMetadataPublicationState::Pending;
        assert!(state.should_probe(
            7,
            Some(PresentModePreference::Auto),
            bevy::window::PresentMode::Fifo,
        ));
        state.await_automatic_immediate(7);
        assert!(!state.should_probe(
            7,
            Some(PresentModePreference::Auto),
            bevy::window::PresentMode::Fifo,
        ));
        assert!(state.should_probe(
            7,
            Some(PresentModePreference::Auto),
            bevy::window::PresentMode::Immediate,
        ));
        state.publish();
        assert!(!state.should_probe(
            7,
            Some(PresentModePreference::Auto),
            bevy::window::PresentMode::Immediate,
        ));
    }

    #[test]
    fn explicit_override_or_replacement_window_releases_deferred_metadata() {
        let mut explicit =
            GraphicsMetadataPublicationState::AwaitingAutomaticImmediate { window: 7 };
        assert!(explicit.should_probe(
            7,
            Some(PresentModePreference::Vsync),
            bevy::window::PresentMode::Fifo,
        ));

        let mut replacement =
            GraphicsMetadataPublicationState::AwaitingAutomaticImmediate { window: 7 };
        assert!(replacement.should_probe(
            8,
            Some(PresentModePreference::Auto),
            bevy::window::PresentMode::Fifo,
        ));
    }

    /// A pre-probe FIFO request on a Mailbox-only surface must not be reported as effective.
    #[test]
    fn metadata_waits_for_the_capability_selected_request() {
        let policy = PresentModePolicy::new(PresentModePreference::NoVsync);
        policy.publish_capabilities(Some(
            SurfacePresentModes::FIFO_ONLY.with(PresentModeKind::Mailbox),
        ));
        let selection = || Some(policy.selection());
        assert!(!selection_applied(selection(), PresentModeKind::Fifo));
        assert!(!selection_applied(selection(), PresentModeKind::Immediate));

        policy.publish_selection(Some(PresentModeKind::Mailbox));
        assert!(!selection_applied(selection(), PresentModeKind::Fifo));
        assert!(selection_applied(selection(), PresentModeKind::Mailbox));
        assert!(selection_applied(None, PresentModeKind::Fifo));
    }
}

use bridge::{SetupState, WorldState, WorldStatus};

/// A step of opening a local world, in the order they run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    CheckingDocker,
    PullingImage,
    DownloadingServer,
    InstallingServer,
    StartingServer,
    Connecting,
}

impl Stage {
    pub fn title(self) -> &'static str {
        match self {
            Self::CheckingDocker => "Checking Docker",
            Self::PullingImage => "Downloading server image",
            Self::DownloadingServer => "Downloading Bedrock Dedicated Server",
            Self::InstallingServer => "Installing Bedrock Dedicated Server",
            Self::StartingServer => "Starting world",
            Self::Connecting => "Joining world",
        }
    }
}

/// What the loading screen shows: a stage, a determinate fraction when one is known, and a detail line.
#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub stage: Stage,
    pub fraction: Option<f32>,
    pub detail: String,
}

impl Progress {
    pub fn connecting(world: &str) -> Self {
        Self {
            stage: Stage::Connecting,
            fraction: None,
            detail: world.to_owned(),
        }
    }
}

fn megabytes(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// The loading-screen state for the last status of a world being opened; `None` status is the
/// moment between the open request and the core's first answer.
pub fn progress(status: Option<&WorldStatus>, world: &str) -> Progress {
    let starting = Progress {
        stage: Stage::StartingServer,
        fraction: None,
        detail: world.to_owned(),
    };
    let Some(status) = status else {
        return starting;
    };
    if status.state == WorldState::Running {
        return Progress::connecting(world);
    }
    // Setup reports the dedicated server's install for every world; only a BDS open drives it.
    let setup = status
        .setup
        .as_ref()
        .filter(|_| status.backend == Some(bridge::Backend::Bds));
    let Some(setup) = setup else {
        return starting;
    };
    match setup.state {
        SetupState::CheckingRuntime => Progress {
            stage: Stage::CheckingDocker,
            fraction: None,
            detail: String::new(),
        },
        SetupState::PullingImage => {
            let (done, total) = (setup.layers_done, setup.layers_total);
            Progress {
                stage: Stage::PullingImage,
                fraction: (total > 0).then(|| done.min(total) as f32 / total as f32),
                detail: if total > 0 {
                    format!("{done} of {total} layers")
                } else {
                    String::new()
                },
            }
        }
        SetupState::Downloading => {
            let (done, total) = (setup.bytes_done, setup.bytes_total);
            Progress {
                stage: Stage::DownloadingServer,
                fraction: (total > 0).then(|| (done.min(total) as f64 / total as f64) as f32),
                detail: if total > 0 {
                    format!("{:.1} / {:.1} MB", megabytes(done), megabytes(total))
                } else {
                    format!("{:.1} MB", megabytes(done))
                },
            }
        }
        SetupState::Unpacking => Progress {
            stage: Stage::InstallingServer,
            fraction: None,
            detail: setup.version.clone().unwrap_or_default(),
        },
        _ => starting,
    }
}

#[cfg(test)]
mod tests {
    use bridge::{Backend, Setup};

    use super::*;

    fn status(backend: Backend, setup: Option<Setup>) -> WorldStatus {
        WorldStatus {
            state: WorldState::Starting,
            world_id: Some("w".to_owned()),
            backend: Some(backend),
            paused: false,
            pause_supported: false,
            error: None,
            setup,
            backend_unavailable_reason: None,
            max_players: None,
        }
    }

    fn setup(state: SetupState) -> Setup {
        Setup {
            state,
            version: Some("1.26.52.3".to_owned()),
            bytes_done: 0,
            bytes_total: 0,
            layers_done: 0,
            layers_total: 0,
            eula_accepted: true,
            error: None,
            runtime: "container".to_owned(),
            reason: None,
        }
    }

    #[test]
    fn every_bds_step_maps_to_its_stage_in_order() {
        let stages: Vec<Stage> = [
            SetupState::CheckingRuntime,
            SetupState::PullingImage,
            SetupState::Downloading,
            SetupState::Unpacking,
            SetupState::Ready,
        ]
        .into_iter()
        .map(|state| progress(Some(&status(Backend::Bds, Some(setup(state)))), "W").stage)
        .collect();
        assert_eq!(
            stages,
            [
                Stage::CheckingDocker,
                Stage::PullingImage,
                Stage::DownloadingServer,
                Stage::InstallingServer,
                Stage::StartingServer,
            ]
        );
        let mut running = status(Backend::Bds, None);
        running.state = WorldState::Running;
        assert_eq!(progress(Some(&running), "W").stage, Stage::Connecting);
        assert_eq!(progress(None, "W").stage, Stage::StartingServer);
    }

    #[test]
    fn downloads_and_pulls_report_a_fraction_only_when_the_total_is_known() {
        let mut download = setup(SetupState::Downloading);
        download.bytes_done = 52_428_800;
        download.bytes_total = 104_857_600;
        let shown = progress(Some(&status(Backend::Bds, Some(download.clone()))), "W");
        assert_eq!(shown.fraction, Some(0.5));
        assert_eq!(shown.detail, "50.0 / 100.0 MB");
        download.bytes_total = 0;
        let unknown = progress(Some(&status(Backend::Bds, Some(download))), "W");
        assert_eq!(
            (unknown.fraction, unknown.detail.as_str()),
            (None, "50.0 MB")
        );

        let mut pull = setup(SetupState::PullingImage);
        pull.layers_done = 3;
        pull.layers_total = 4;
        let shown = progress(Some(&status(Backend::Bds, Some(pull))), "W");
        assert_eq!(shown.fraction, Some(0.75));
        assert_eq!(shown.detail, "3 of 4 layers");
    }

    /// A dragonfly world never shows the dedicated server's download, even while one is reported.
    #[test]
    fn a_dragonfly_open_ignores_bds_setup() {
        let flat = status(Backend::Dragonfly, Some(setup(SetupState::Downloading)));
        assert_eq!(progress(Some(&flat), "Flat").stage, Stage::StartingServer);
    }
}

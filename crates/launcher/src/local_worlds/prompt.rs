use super::model::Input;

pub const DOCKER_URL: &str = "https://www.docker.com/products/docker-desktop/";

/// Why BDS cannot run in Docker on macOS.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromptKind {
    DockerMissing,
    DockerNotRunning,
}

/// What the Docker modal is blocking.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromptFor {
    /// Creating a world on the dedicated server.
    CreateBds,
    /// Playing a saved world that runs on the dedicated server.
    Play,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromptButton {
    /// Create using Dragonfly while preserving the terrain choice.
    UseDragonfly,
    GetDocker,
    Retry,
    Cancel,
}

/// The Docker modal: its reason and what it blocks decide the text and the ways forward.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Prompt {
    pub kind: PromptKind,
    pub blocking: PromptFor,
}

impl Prompt {
    pub fn title(self) -> &'static str {
        match self.kind {
            PromptKind::DockerMissing => "Docker is needed",
            PromptKind::DockerNotRunning => "Docker is not running",
        }
    }

    pub fn text(self) -> String {
        let subject = match self.blocking {
            PromptFor::Play => "This world runs",
            PromptFor::CreateBds => "Worlds run",
        };
        let remedy = match self.kind {
            PromptKind::DockerMissing => {
                "Install a free Docker app such as Docker Desktop, OrbStack or Colima, start it, \
                 then choose Retry."
            }
            PromptKind::DockerNotRunning => {
                "Docker is installed but not running. Start Docker Desktop, OrbStack or Colima, \
                 then choose Retry."
            }
        };
        format!(
            "{subject} on Minecraft's official Bedrock Dedicated Server, which only runs on \
             Windows and Linux, so on Mac it runs inside Docker. {remedy}"
        )
    }

    /// The first button is the modal's primary action.
    pub fn buttons(self) -> &'static [PromptButton] {
        use PromptButton::*;
        match self.kind {
            PromptKind::DockerMissing => &[GetDocker, Retry, Cancel],
            PromptKind::DockerNotRunning => &[Retry, Cancel],
        }
    }
}

impl PromptButton {
    pub fn label(self) -> String {
        match self {
            Self::UseDragonfly => "Use Dragonfly".to_owned(),
            Self::GetDocker => "Get Docker".to_owned(),
            Self::Retry => "Retry".to_owned(),
            Self::Cancel => "Cancel".to_owned(),
        }
    }

    pub fn input(self) -> Input {
        Input::Prompt(self)
    }
}

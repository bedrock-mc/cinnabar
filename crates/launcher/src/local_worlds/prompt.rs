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
        match (self.kind, self.blocking) {
            (PromptKind::DockerMissing, PromptFor::Play) => {
                "This world runs on the official Bedrock Dedicated Server, which needs Docker on Mac \
                 (Docker Desktop, OrbStack or Colima). Install Docker, start it, then play again."
                    .to_owned()
            }
            (PromptKind::DockerNotRunning, PromptFor::Play) => {
                "This world runs on the official Bedrock Dedicated Server in Docker. Start Docker, \
                 then choose Retry."
                    .to_owned()
            }
            (PromptKind::DockerMissing, _) => "BDS needs Docker on Mac (Docker Desktop, OrbStack or Colima). Install Docker, or use Dragonfly with the same world generator.".to_owned(),
            (PromptKind::DockerNotRunning, _) => "BDS needs Docker to be running. Start Docker and retry, or use Dragonfly with the same world generator.".to_owned(),
        }
    }

    pub fn buttons(self) -> &'static [PromptButton] {
        use PromptButton::*;
        match (self.kind, self.blocking) {
            (PromptKind::DockerMissing, PromptFor::CreateBds) => &[UseDragonfly, GetDocker, Cancel],
            (PromptKind::DockerMissing, PromptFor::Play) => &[GetDocker, Cancel],
            (PromptKind::DockerNotRunning, PromptFor::Play) => &[Retry, Cancel],
            (PromptKind::DockerNotRunning, _) => &[Retry, UseDragonfly, Cancel],
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

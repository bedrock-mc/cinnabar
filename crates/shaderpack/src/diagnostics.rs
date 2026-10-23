//! Translation diagnostics, located in the pack's own files.

use std::fmt;

/// A pack file position: shaders-root-relative path with forward slashes and a 1-based line.
#[derive(
    serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub struct SourceLocation {
    pub file: String,
    pub line: u32,
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.file, self.line)
    }
}

/// Which translation step reported a problem.
#[derive(
    serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub enum Phase {
    /// Reading a stage file or one of its `#include`s.
    Include,
    /// Macro expansion and conditional compilation.
    Preprocess,
    /// The compatibility-profile to GLSL 4.50 rewrite.
    Transform,
    /// naga's GLSL frontend.
    Parse,
    /// naga IR validation.
    Validate,
    /// Agreement between a program's stages and its pack directives.
    Link,
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Phase::Include => "include",
            Phase::Preprocess => "preprocess",
            Phase::Transform => "transform",
            Phase::Parse => "parse",
            Phase::Validate => "validate",
            Phase::Link => "link",
        })
    }
}

/// One error. `location` is the pack file and line the problem maps back to through
/// `#include` expansion; `None` for code the translator generated or program-level problems.
/// `snippet` is the offending translated GLSL line when the location is generated code.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub phase: Phase,
    pub message: String,
    pub location: Option<SourceLocation>,
    pub snippet: Option<String>,
}

impl Diagnostic {
    /// Creates an unlocated diagnostic for one translation phase.
    pub(crate) fn new(phase: Phase, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            phase,
            message: message.into(),
            location: None,
            snippet: None,
        }
    }

    /// Attaches the pack location when the source is known.
    pub(crate) fn at(mut self, location: Option<SourceLocation>) -> Diagnostic {
        self.location = location;
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.location {
            Some(loc) => write!(f, "{loc}: {}: {}", self.phase, self.message)?,
            None => write!(f, "{}: {}", self.phase, self.message)?,
        }
        if let Some(s) = &self.snippet {
            write!(f, " (in `{}`)", s.trim())?;
        }
        Ok(())
    }
}

//! Developer audit of the uniforms a pack receives. With `CINNABAR_SHADERPACK_UNIFORM_DUMP`
//! set to a file path, every view rewrites that file once a second with each program's and
//! geometry draw's uniform members and the value packed for them, flagging members the host
//! does not know (`UNKNOWN`, packed as zero) and known members whose value is all zero
//! (`zero`), then the custom uniforms and the built-in inputs their expressions read that
//! the host does not supply (evaluated as 0).

use super::uniforms::HostValue;
use shaderpack::{UniformBlock, UniformValue};
use std::fmt::Write as _;
use std::path::PathBuf;

/// Names the dump file; unset disables the audit.
pub(crate) const DUMP_ENV: &str = "CINNABAR_SHADERPACK_UNIFORM_DUMP";
/// Frames between rewrites of the dump.
const DUMP_INTERVAL_FRAMES: u64 = 60;

/// One view's dump target and the text of the dump being assembled.
pub(crate) struct UniformAudit {
    path: PathBuf,
    text: String,
    writing: bool,
}

impl UniformAudit {
    /// The audit named by [`DUMP_ENV`], or `None` when it is unset or empty.
    pub(crate) fn from_env() -> Option<Self> {
        let path = std::env::var_os(DUMP_ENV).filter(|path| !path.is_empty())?;
        Some(Self {
            path: PathBuf::from(path),
            text: String::new(),
            writing: false,
        })
    }

    /// Starts a dump when `frame` is due and returns whether it did; the other calls this
    /// frame append to it.
    pub(crate) fn begin(&mut self, pack: &str, frame: u64) -> bool {
        self.writing = frame % DUMP_INTERVAL_FRAMES == 1;
        if self.writing {
            self.text.clear();
            let _ = writeln!(self.text, "# shader pack {pack}, frame {frame}");
        }
        self.writing
    }

    /// Appends one uniform block: every member with the value `lookup` gives it.
    pub(crate) fn block(
        &mut self,
        title: &str,
        block: &UniformBlock,
        lookup: &dyn Fn(&str) -> Option<HostValue>,
    ) {
        if !self.writing {
            return;
        }
        let _ = writeln!(self.text, "\n## {title} ({} bytes)", block.size);
        for member in &block.members {
            let value = match member.array {
                Some(_) => lookup(&format!("{}[0]", member.name)).or_else(|| lookup(&member.name)),
                None => lookup(&member.name),
            };
            let _ = match value {
                Some(value) => writeln!(
                    self.text,
                    "  {:<34} {:?} = {}{}",
                    member.name,
                    member.ty,
                    describe(&value),
                    if is_zero(&value) { "  zero" } else { "" }
                ),
                None => writeln!(self.text, "  {:<34} {:?} = UNKNOWN", member.name, member.ty),
            };
        }
    }

    /// Appends the custom uniforms' values and the inputs the host does not supply, then
    /// writes the finished dump.
    pub(crate) fn finish<'a>(
        &mut self,
        custom: &[(String, UniformValue)],
        missing_inputs: impl Iterator<Item = &'a str>,
    ) {
        if !self.writing {
            return;
        }
        self.writing = false;
        let _ = writeln!(self.text, "\n## custom uniforms");
        for (name, value) in custom {
            let _ = writeln!(self.text, "  {name:<34} = {value:?}");
        }
        let _ = writeln!(
            self.text,
            "\n## custom-uniform inputs without a host value (0)"
        );
        for name in missing_inputs {
            let _ = writeln!(self.text, "  {name}");
        }
        if let Err(error) = std::fs::write(&self.path, &self.text) {
            bevy::log::warn!(
                target: "cinnabar::shaderpack",
                path = %self.path.display(), "uniform dump not written: {error}"
            );
        }
    }
}

/// A compact rendering of `value`; matrices print column by column.
fn describe(value: &HostValue) -> String {
    match *value {
        HostValue::Float(v) => format!("{v}"),
        HostValue::Int(v) => format!("{v}"),
        HostValue::Vec(v, n) => format!("{:?}", &v.to_array()[..usize::from(n)]),
        HostValue::IVec(v, n) => format!("{:?}", &v[..usize::from(n)]),
        HostValue::Mat3(m) => format!("{:?}", m.to_cols_array_2d()),
        HostValue::Mat4(m) => format!("{:?}", m.to_cols_array_2d()),
    }
}

fn is_zero(value: &HostValue) -> bool {
    match *value {
        HostValue::Float(v) => v == 0.0,
        HostValue::Int(v) => v == 0,
        HostValue::Vec(v, n) => v.to_array()[..usize::from(n)].iter().all(|c| *c == 0.0),
        HostValue::IVec(v, n) => v[..usize::from(n)].iter().all(|c| *c == 0),
        HostValue::Mat3(m) => m.to_cols_array().iter().all(|c| *c == 0.0),
        HostValue::Mat4(m) => m.to_cols_array().iter().all(|c| *c == 0.0),
    }
}

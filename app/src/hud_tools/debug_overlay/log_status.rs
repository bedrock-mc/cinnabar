//! The F3 row naming the active log filter and whether new sessions record a packet capture.

use std::ffi::OsString;
use std::fmt::{Display, Write};

use bevy::log::tracing_subscriber::filter::LevelFilter;
use diagnostics::logging::Category;

use crate::runtime::network::session::{CaptureChoice, packet_capture_choice};

/// What the row shows; it is formatted again only when these change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Inputs {
    filter_generation: u64,
    capture: CaptureChoice,
}

/// The environment the row reads once: the process environment does not change while running.
struct Environment {
    capture: Option<OsString>,
    rust_log: bool,
}

impl Environment {
    fn read() -> Self {
        Self {
            capture: std::env::var_os(client_session::markers::PACKET_CAPTURE),
            rust_log: std::env::var_os("RUST_LOG").is_some_and(|value| !value.is_empty()),
        }
    }
}

/// The retained row and the inputs it was formatted from.
#[derive(Default)]
pub(super) struct LogStatus {
    inputs: Option<Inputs>,
    environment: Option<Environment>,
    line: String,
    /// Times the row was formatted, for cost tests.
    #[cfg(test)]
    pub(super) formats: usize,
}

impl LogStatus {
    /// The row for the current filter and the Record network session setting. Unchanged inputs
    /// cost one atomic load and return the retained row without formatting.
    pub(super) fn refresh(&mut self, record_setting: bool) -> &str {
        let environment = self.environment.get_or_insert_with(Environment::read);
        let inputs = Inputs {
            filter_generation: diagnostics::logging::filter_generation(),
            capture: packet_capture_choice(environment.capture.as_deref(), record_setting),
        };
        if self.inputs != Some(inputs) {
            self.inputs = Some(inputs);
            let filter = diagnostics::logging::filter();
            write_row(
                &mut self.line,
                filter.as_ref().map(|filter| filter.spec.as_str()),
                environment.rust_log,
                diagnostics::logging::category_filter,
                inputs.capture,
            );
            #[cfg(test)]
            {
                self.formats += 1;
            }
        }
        &self.line
    }
}

/// Writes `Log: <filter> | Capture: <capture>`. The filter is `custom` when `RUST_LOG` is set
/// or `spec` holds a directive other than a category's; otherwise it lists the categories
/// `level` puts at other than `info`, or is `info` when none are. `spec` is the layered
/// `CINNABAR_LOG`-style spec, `None` before logging is installed.
fn write_row(
    line: &mut String,
    spec: Option<&str>,
    rust_log: bool,
    level: impl Fn(Category) -> LevelFilter,
    capture: impl Display,
) {
    line.clear();
    line.push_str("Log: ");
    let custom = rust_log
        || spec.is_some_and(|spec| {
            spec.split(',')
                .map(str::trim)
                .filter(|directive| !directive.is_empty())
                .any(|directive| {
                    let name = directive
                        .split_once('=')
                        .map_or(directive, |(name, _)| name);
                    Category::from_name(name).is_none()
                })
        });
    if spec.is_none() {
        line.push_str("not installed");
    } else if custom {
        line.push_str("custom");
    } else {
        let start = line.len();
        for category in Category::ALL {
            let level = level(category);
            if level != LevelFilter::INFO {
                if line.len() > start {
                    line.push(' ');
                }
                write!(line, "{}={}", category.name(), level_name(level)).unwrap();
            }
        }
        if line.len() == start {
            line.push_str("info");
        }
    }
    write!(line, " | Capture: {capture}").unwrap();
}

/// A level's lowercase spec name.
fn level_name(level: LevelFilter) -> &'static str {
    [
        (LevelFilter::TRACE, "trace"),
        (LevelFilter::DEBUG, "debug"),
        (LevelFilter::INFO, "info"),
        (LevelFilter::WARN, "warn"),
        (LevelFilter::ERROR, "error"),
    ]
    .into_iter()
    .find_map(|(known, name)| (known == level).then_some(name))
    .unwrap_or("off")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(spec: Option<&str>, rust_log: bool, capture: &str) -> String {
        let verbose = [
            Category::Net,
            Category::Session,
            Category::Core,
            Category::Packs,
        ];
        let mut line = String::new();
        write_row(
            &mut line,
            spec,
            rust_log,
            |category| {
                if spec.is_some_and(|spec| spec.contains("=debug")) && verbose.contains(&category) {
                    LevelFilter::DEBUG
                } else {
                    LevelFilter::INFO
                }
            },
            capture,
        );
        line
    }

    #[test]
    fn the_row_names_category_levels_or_custom_and_the_capture_choice() {
        assert_eq!(
            row(
                Some("net=debug,session=debug,core=debug,packs=debug"),
                false,
                "on"
            ),
            "Log: net=debug packs=debug core=debug session=debug | Capture: on"
        );
        assert_eq!(row(Some(""), false, "off"), "Log: info | Capture: off");
        assert_eq!(
            row(Some("net=debug,wgpu_core=trace"), false, "off"),
            "Log: custom | Capture: off"
        );
        let variable = client_session::markers::PACKET_CAPTURE;
        assert_eq!(
            row(Some(""), true, &format!("on ({variable})")),
            format!("Log: custom | Capture: on ({variable})")
        );
        assert_eq!(row(None, false, "off"), "Log: not installed | Capture: off");
    }

    #[test]
    fn the_row_is_formatted_again_only_when_its_inputs_change() {
        let mut status = LogStatus {
            environment: Some(Environment {
                capture: None,
                rust_log: false,
            }),
            ..LogStatus::default()
        };
        assert!(status.refresh(false).ends_with("| Capture: off"));
        status.refresh(false);
        status.refresh(false);
        assert_eq!(status.formats, 1, "unchanged inputs keep the retained row");
        assert!(status.refresh(true).ends_with("| Capture: on"));
        assert_eq!(status.formats, 2);
        status.refresh(true);
        assert_eq!(status.formats, 2);

        let mut environment = LogStatus {
            environment: Some(Environment {
                capture: Some("0".into()),
                rust_log: false,
            }),
            ..LogStatus::default()
        };
        assert!(environment.refresh(true).ends_with(&format!(
            "| Capture: off ({})",
            client_session::markers::PACKET_CAPTURE
        )));
        environment.refresh(false);
        assert_eq!(
            environment.formats, 1,
            "the setting cannot change what the variable decided"
        );
    }
}

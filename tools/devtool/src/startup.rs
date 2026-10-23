//! Release launch acceptance with presentation-based samples and separate cache modes.

use crate::DevtoolError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

const WARM_CACHE: &str = "warm";
const COLD_CACHE: &str = "cold";
/// Each supported driver override owns its isolated directory name.
const DRIVER_CACHE_ENVIRONMENT: [(&str, &str); 2] = [
    ("MESA_SHADER_CACHE_DIR", "mesa"),
    ("__GL_SHADER_DISK_CACHE_PATH", "gl"),
];

#[derive(Debug)]
struct OwnedClient(std::process::Child);

impl std::ops::Deref for OwnedClient {
    type Target = std::process::Child;
    /// Borrows the exact child owned by this acceptance run.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for OwnedClient {
    /// Keeps process operations tied to the child handle rather than process-name searches.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for OwnedClient {
    /// Reaps the owned client even when parsing or waiting returns early.
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

#[derive(Debug)]
struct Options {
    client: PathBuf,
    runs: usize,
    modes: Vec<&'static str>,
    budget_ms: f64,
    cold_budget_ms: f64,
    out: Option<PathBuf>,
    baseline: Option<PathBuf>,
    cold_reset: Option<PathBuf>,
    args: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Summary {
    modes: Vec<Mode>,
}

#[derive(Serialize, Deserialize)]
struct Mode {
    cache: String,
    budget_ms: f64,
    menu: Distribution,
    stages: BTreeMap<String, Distribution>,
    samples: Vec<Value>,
}

#[derive(Serialize, Deserialize)]
struct Distribution {
    median_ms: f64,
    p99_ms: f64,
}

/// Parses an explicit executable and forwards arguments after `--` unchanged.
fn parse(args: &[String]) -> Result<Options, DevtoolError> {
    let mut options = Options {
        client: PathBuf::from("target/release/cinnabar"),
        runs: 30,
        modes: vec![WARM_CACHE],
        budget_ms: 1000.0,
        cold_budget_ms: 4000.0,
        out: None,
        baseline: None,
        cold_reset: None,
        args: Vec::new(),
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            options.args = args.cloned().collect();
            break;
        }
        let value = args
            .next()
            .ok_or_else(|| error(format!("{arg} requires a value")))?;
        match arg.as_str() {
            "--client" => options.client = value.into(),
            "--runs" => options.runs = value.parse().map_err(|_| error("invalid run count"))?,
            "--cache" => {
                options.modes = match value.as_str() {
                    WARM_CACHE => vec![WARM_CACHE],
                    COLD_CACHE => vec![COLD_CACHE],
                    "both" => vec![WARM_CACHE, COLD_CACHE],
                    _ => return Err(error("--cache must be warm, cold or both")),
                }
            }
            "--budget-ms" => options.budget_ms = positive(value)?,
            "--cold-budget-ms" => options.cold_budget_ms = positive(value)?,
            "--out" => options.out = Some(value.into()),
            "--baseline" => options.baseline = Some(value.into()),
            "--cold-reset" => options.cold_reset = Some(value.into()),
            _ => return Err(error(format!("unknown startup option {arg}"))),
        }
    }
    if options.runs == 0 {
        return Err(error("--runs must be positive"));
    }
    if options
        .client
        .parent()
        .is_some_and(|parent| parent.file_name().is_some_and(|name| name == "debug"))
    {
        return Err(error("startup acceptance requires a release client"));
    }
    if cfg!(any(target_os = "macos", target_os = "windows"))
        && options.modes.contains(&COLD_CACHE)
        && options.cold_reset.is_none()
    {
        return Err(error(
            "cold cache runs require --cold-reset on this platform",
        ));
    }
    Ok(options)
}

/// Rejects NaN, infinity and nonpositive budgets before any client is launched.
fn positive(value: &str) -> Result<f64, DevtoolError> {
    let value: f64 = value.parse().map_err(|_| error("invalid budget"))?;
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(error("budget must be finite and positive"))
    }
}

/// Gives startup command failures their own searchable prefix.
fn error(message: impl std::fmt::Display) -> DevtoolError {
    DevtoolError::Usage(format!("startup: {message}"))
}

/// Launches one owned process and waits for its structured report and orderly exit.
fn launch(options: &Options, cache: &Path) -> Result<Value, DevtoolError> {
    let mut command = Command::new(&options.client);
    command
        .args(&options.args)
        .env(render_api::STARTUP_BENCHMARK_ENV, "1")
        .env("NO_COLOR", "1");
    for (variable, directory) in DRIVER_CACHE_ENVIRONMENT {
        let directory = cache.join(directory);
        std::fs::create_dir_all(&directory).map_err(error)?;
        command.env(variable, directory);
    }
    let child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| DevtoolError::Spawn {
            command: options.client.display().to_string(),
            source,
        })?;
    let mut child = OwnedClient(child);
    let (send, lines) = mpsc::channel();
    let mut readers = Vec::new();
    for pipe in [
        Box::new(child.stdout.take().unwrap()) as Box<dyn std::io::Read + Send>,
        Box::new(child.stderr.take().unwrap()),
    ] {
        let send = send.clone();
        readers.push(std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                if send.send(line).is_err() {
                    break;
                }
            }
        }));
    }
    drop(send);
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut report = None;
    let mut tail = std::collections::VecDeque::new();
    let result = loop {
        if let Ok(Some(status)) = child.try_wait() {
            // Readers may still be draining the process's final report.
            for reader in readers.drain(..) {
                let _ = reader.join();
            }
            for line in lines.try_iter() {
                observe(&line, &mut report, &mut tail)?;
            }
            break if status.success() {
                report.ok_or_else(|| error("client exited without a presented menu report; build with acceptance enabled"))
            } else {
                Err(error(format!(
                    "client exited {status}: {}",
                    tail.into_iter().collect::<Vec<_>>().join("\n")
                )))
            };
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            let _ = child.kill();
            let _ = child.wait();
            break Err(error(
                "client did not present a menu and exit within the launch watchdog",
            ));
        }
        match lines.recv_timeout(left.min(Duration::from_millis(50))) {
            Ok(line) => observe(&line, &mut report, &mut tail)?,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // No output remains; wait on the process without spinning on the closed channel.
                std::thread::park_timeout(left.min(Duration::from_millis(50)));
            }
        }
    };
    for reader in readers {
        let _ = reader.join();
    }
    result
}

/// Retains a bounded failure tail and accepts only a finite presentation measurement.
fn observe(
    line: &str,
    report: &mut Option<Value>,
    tail: &mut std::collections::VecDeque<String>,
) -> Result<(), DevtoolError> {
    if let Some((_, json)) = line.split_once(render_api::STARTUP_TIMING_MARKER) {
        let parsed: Value = serde_json::Deserializer::from_str(json.trim())
            .into_iter()
            .next()
            .ok_or_else(|| error("empty startup report"))?
            .map_err(|failure| error(format!("invalid startup report: {failure}")))?;
        measurement(&parsed, "menu_ms")?;
        if parsed["debug_assertions"].as_bool() != Some(false) {
            return Err(error("startup acceptance requires a release client report"));
        }
        if report.is_some() {
            return Err(error("client emitted more than one startup report"));
        }
        if !parsed["stages_ms"].is_object() {
            return Err(error("startup report has no stage table"));
        }
        *report = Some(parsed);
    }
    if tail.len() == 20 {
        tail.pop_front();
    }
    tail.push_back(line.into());
    Ok(())
}

/// Reads a duration without accepting missing, negative or nonnumeric measurements.
fn measurement(sample: &Value, key: &str) -> Result<f64, DevtoolError> {
    sample[key]
        .as_f64()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| error(format!("missing or invalid {key}")))
}

/// Uses the nearest-rank p99 and the middle pair for an even-sized median.
fn distribution(mut samples: Vec<f64>) -> Distribution {
    samples.sort_by(f64::total_cmp);
    let middle = samples.len() / 2;
    let median_ms = if samples.len().is_multiple_of(2) {
        (samples[middle - 1] + samples[middle]) / 2.0
    } else {
        samples[middle]
    };
    let index = (samples.len() * 99).div_ceil(100) - 1;
    Distribution {
        median_ms,
        p99_ms: samples[index],
    }
}

/// Summarizes all stages the client emitted, without a duplicated stage list.
fn summarize(cache: &str, budget_ms: f64, samples: Vec<Value>) -> Result<Mode, DevtoolError> {
    if samples.is_empty() {
        return Err(error("no launch samples"));
    }
    let menu = distribution(
        samples
            .iter()
            .map(|sample| measurement(sample, "menu_ms"))
            .collect::<Result<_, _>>()?,
    );
    let mut stages: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for sample in &samples {
        let mut times = sample["stages_ms"]
            .as_object()
            .ok_or_else(|| error("missing stage table"))?
            .clone();
        if let Some(frames) = sample["render_frames"].as_array() {
            for (index, frame) in frames.iter().enumerate() {
                if let Some(frame) = frame.as_object() {
                    for (name, duration) in frame.iter().filter(|(name, _)| name.ends_with("_ms")) {
                        times.insert(format!("render_{}_{name}", index + 1), duration.clone());
                    }
                }
            }
        }
        if let Some(updates) = sample["main_updates_ms"].as_array() {
            for (index, duration) in updates.iter().enumerate() {
                times.insert(format!("main_update_{}", index + 1), duration.clone());
            }
        }
        for (name, value) in times {
            let value = value
                .as_f64()
                .filter(|value| value.is_finite() && *value >= 0.0)
                .ok_or_else(|| error(format!("invalid stage {name}")))?;
            stages.entry(name).or_default().push(value);
        }
    }
    Ok(Mode {
        cache: cache.into(),
        budget_ms,
        menu,
        stages: stages
            .into_iter()
            .map(|(name, values)| (name, distribution(values)))
            .collect(),
        samples,
    })
}

/// Reports every stage's movement against the same cache mode in a saved acceptance result.
fn growth(mode: &Mode, baseline: Option<&Mode>) -> String {
    mode.stages
        .iter()
        .filter_map(|(name, current)| {
            let previous = baseline?
                .stages
                .get(name)
                .map_or(0.0, |previous| previous.p99_ms);
            let delta = current.p99_ms - previous;
            (delta > 0.0).then(|| format!("{name} +{delta:.2} ms"))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Runs warm and cold driver-cache launches and enforces their p99 budgets.
pub(crate) fn run(args: &[String]) -> Result<(), DevtoolError> {
    let options = parse(args)?;
    let caches = tempfile::tempdir().map_err(error)?;
    let baseline: Option<Summary> = options
        .baseline
        .as_ref()
        .map(|path| {
            let bytes = std::fs::read(path).map_err(error)?;
            serde_json::from_slice(&bytes).map_err(error)
        })
        .transpose()?;
    let mut summary = Summary { modes: Vec::new() };
    for &cache in &options.modes {
        let warm = caches.path().join(WARM_CACHE);
        if cache == WARM_CACHE {
            launch(&options, &warm)?;
        }
        let mut samples = Vec::with_capacity(options.runs);
        for index in 0..options.runs {
            let directory = if cache == WARM_CACHE {
                warm.clone()
            } else {
                caches.path().join(format!("cold-{index}"))
            };
            if cache == COLD_CACHE
                && let Some(reset) = &options.cold_reset
            {
                let status =
                    Command::new(reset)
                        .status()
                        .map_err(|source| DevtoolError::Spawn {
                            command: reset.display().to_string(),
                            source,
                        })?;
                if !status.success() {
                    return Err(error(format!("cold cache reset exited {status}")));
                }
            }
            samples.push(launch(&options, &directory)?);
        }
        let budget = if cache == WARM_CACHE {
            options.budget_ms
        } else {
            options.cold_budget_ms
        };
        summary.modes.push(summarize(cache, budget, samples)?);
    }
    if let Some(path) = &options.out {
        std::fs::write(path, serde_json::to_vec_pretty(&summary).map_err(error)?).map_err(error)?;
    }
    let mut failed = false;
    for mode in &summary.modes {
        println!(
            "{} cache: n={} menu median={:.2} ms p99={:.2} ms budget={:.2} ms",
            mode.cache,
            mode.samples.len(),
            mode.menu.median_ms,
            mode.menu.p99_ms,
            mode.budget_ms
        );
        println!("stage\tmedian ms\tp99 ms");
        for (name, sample) in &mode.stages {
            println!("{name}\t{:.2}\t{:.2}", sample.median_ms, sample.p99_ms);
        }
        if mode.menu.p99_ms > mode.budget_ms {
            failed = true;
            let prior = baseline
                .as_ref()
                .and_then(|summary| summary.modes.iter().find(|prior| prior.cache == mode.cache));
            let grew = growth(mode, prior);
            eprintln!(
                "{} startup exceeds budget; stage growth: {}",
                mode.cache,
                if grew.is_empty() {
                    "no comparable baseline; use --baseline to attribute growth"
                } else {
                    &grew
                }
            );
        }
    }
    if failed {
        Err(error("launch-to-menu p99 exceeds budget"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantiles_and_stage_growth_use_samples_rather_than_wall_clock() {
        let values = (1..=100).map(|n| n as f64).collect();
        let result = distribution(values);
        assert_eq!(result.median_ms, 50.5);
        assert_eq!(result.p99_ms, 99.0);
        let sample =
            |menu, stage| serde_json::json!({"menu_ms": menu, "stages_ms": {"device": stage}});
        let before = summarize(WARM_CACHE, 10.0, vec![sample(5.0, 2.0)]).unwrap();
        let after = summarize(WARM_CACHE, 10.0, vec![sample(11.0, 4.0)]).unwrap();
        assert!(after.menu.p99_ms > after.budget_ms);
        assert_eq!(growth(&after, Some(&before)), "device +2.00 ms");
    }

    #[test]
    fn invalid_budgets_and_reports_cannot_pass_acceptance() {
        for value in ["NaN", "inf", "0", "-1"] {
            assert!(positive(value).is_err());
        }
        let cold = ["--cache".to_owned(), COLD_CACHE.to_owned()];
        assert_eq!(
            parse(&cold).is_err(),
            cfg!(any(target_os = "macos", target_os = "windows"))
        );
        let mut reset = cold.to_vec();
        reset.extend(["--cold-reset".to_owned(), "reset-fixture".to_owned()]);
        assert!(parse(&reset).is_ok());
        let mut report = None;
        let mut tail = Default::default();
        let line = format!(
            "{} {{\"stages_ms\":{{}}}}",
            render_api::STARTUP_TIMING_MARKER
        );
        assert!(observe(&line, &mut report, &mut tail).is_err());
        assert!(summarize(WARM_CACHE, 1.0, vec![serde_json::json!({"menu_ms": -1})]).is_err());
    }
}

#[cfg(all(test, unix))]
mod process_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn launch_requires_a_presented_report_and_successful_process_exit() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("release-client-fixture");
        let options = Options {
            client: executable.clone(),
            ..parse(&[]).unwrap()
        };
        let report = serde_json::json!({"debug_assertions": false, "menu_ms": 7, "stages_ms": {"device": 2}});
        for (line, code, succeeds) in [
            (
                format!("{} {report}", render_api::STARTUP_TIMING_MARKER),
                0,
                true,
            ),
            (String::new(), 0, false),
            (
                format!("{} {report}", render_api::STARTUP_TIMING_MARKER),
                1,
                false,
            ),
        ] {
            std::fs::write(
                &executable,
                format!("#!/bin/sh\nprintf '%s\\n' '{line}' >&2\nexit {code}\n"),
            )
            .unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            let result = launch(&options, directory.path());
            for (_, name) in DRIVER_CACHE_ENVIRONMENT {
                assert!(directory.path().join(name).is_dir());
            }
            assert_eq!(result.is_ok(), succeeds, "{result:?}");
            if succeeds {
                assert_eq!(result.unwrap()["menu_ms"], 7);
            }
        }
    }
}

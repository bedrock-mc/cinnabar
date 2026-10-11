//! Tracy export, frame-interval statistics, and comparable baseline summaries.
use crate::DevtoolError;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Zone {
    name: String,
    #[serde(default)]
    source: String,
    total_ms: f64,
    max_ms: f64,
}
#[derive(Debug, Serialize, Deserialize)]
struct Summary {
    refresh_hz: f64,
    frame_zone: String,
    frames: usize,
    p50_ms: f64,
    p90_ms: f64,
    p99_ms: f64,
    hitches: usize,
    zones: Vec<Zone>,
}
#[derive(Debug)]
struct Options {
    seconds: u32,
    baseline: Option<PathBuf>,
    refresh_hz: f64,
    frame_zone: String,
    output: PathBuf,
}

/// Captures the developer client or exports an existing trace; never chooses a server to join.
pub(crate) fn run(args: &[String], summary_only: bool) -> Result<(), DevtoolError> {
    let (options, trace) = parse(args, summary_only)?;
    fs::create_dir_all(&options.output).map_err(io_error)?;
    let trace = if let Some(trace) = trace {
        trace
    } else {
        // Check tools before a costly build or client launch.
        if Command::new("tracy-capture")
            .arg("--help")
            .output()
            .is_err()
            || Command::new("tracy-csvexport")
                .arg("--help")
                .output()
                .is_err()
        {
            return Err(DevtoolError::Usage(
                "install tracy-capture and tracy-csvexport before capturing".into(),
            ));
        }
        checked(Command::new("make").args(["play-build", "TRACY=1"]))?;
        let profile = std::env::var("PROFILE").unwrap_or_else(|_| "play".into());
        let directory = if profile == "dev" { "debug" } else { &profile };
        let executable = PathBuf::from("target")
            .join(directory)
            .join(if cfg!(windows) {
                "bedrock-client.exe"
            } else {
                "bedrock-client"
            });
        let port = capture_port()?.to_string();
        let mut client = OwnedChild(
            Command::new(executable)
                .env("TRACY_PORT", &port)
                .spawn()
                .map_err(io_error)?,
        );
        let trace = options.output.join("capture.tracy");
        let mut collector = OwnedChild(
            Command::new("tracy-capture")
                .args([
                    "-a",
                    CAPTURE_ADDRESS,
                    "-p",
                    &port,
                    "-s",
                    &options.seconds.to_string(),
                    "-o",
                ])
                .arg(&trace)
                .spawn()
                .map_err(io_error)?,
        );
        let captured = wait_for_capture(
            &mut client,
            &mut collector,
            Duration::from_secs(u64::from(options.seconds)) + CONNECTION_ALLOWANCE,
        );
        client.stop()?;
        collector.stop()?;
        captured?;
        trace
    };
    let summary = export(&trace, &options)?;
    print_summary(&summary);
    if let Some(baseline) = &options.baseline {
        let baseline = if baseline.extension().is_some_and(|ext| ext == "tracy") {
            let directory = options.output.join("baseline");
            fs::create_dir_all(&directory).map_err(io_error)?;
            let base_options = Options {
                output: directory,
                ..options.clone_for_export()
            };
            export(baseline, &base_options)?
        } else {
            serde_json::from_slice::<Summary>(&fs::read(baseline).map_err(io_error)?)?
        };
        print_diff(&baseline, &summary)?;
    }
    let path = options.output.join("summary.json");
    fs::write(&path, serde_json::to_vec_pretty(&summary)?).map_err(io_error)?;
    println!("summary: {}", path.display());
    Ok(())
}

impl Options {
    /// Copies only export settings so a baseline cannot recursively compare itself.
    fn clone_for_export(&self) -> Self {
        Self {
            seconds: self.seconds,
            baseline: None,
            refresh_hz: self.refresh_hz,
            frame_zone: self.frame_zone.clone(),
            output: self.output.clone(),
        }
    }
}

/// Requires positive durations and an explicit refresh rate or a reported 60 Hz default.
fn parse(args: &[String], summary_only: bool) -> Result<(Options, Option<PathBuf>), DevtoolError> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut options = Options {
        seconds: 180,
        baseline: None,
        refresh_hz: 60.0,
        frame_zone: "present_frames".into(),
        output: PathBuf::from(format!(".local/captures/capture-{stamp}")),
    };
    let mut trace = None;
    let mut explicit_hz = false;
    let mut arguments = args.iter();
    while let Some(flag) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| DevtoolError::Usage(format!("{flag} requires a value")))?;
        match flag.as_str() {
            "--seconds" => {
                options.seconds = value
                    .parse()
                    .map_err(|_| DevtoolError::Usage("invalid seconds".into()))?
            }
            "--baseline" => options.baseline = Some(value.into()),
            "--refresh-hz" => {
                options.refresh_hz = value
                    .parse()
                    .map_err(|_| DevtoolError::Usage("invalid refresh rate".into()))?;
                explicit_hz = true;
            }
            "--frame-zone" => options.frame_zone = value.clone(),
            "--out" => options.output = value.into(),
            "--trace" if summary_only => trace = Some(PathBuf::from(value)),
            _ => {
                return Err(DevtoolError::Usage(format!(
                    "unknown capture argument: {flag}"
                )));
            }
        }
    }
    if options.seconds == 0 || !options.refresh_hz.is_finite() || options.refresh_hz <= 0.0 {
        return Err(DevtoolError::Usage(
            "seconds and refresh rate must be positive".into(),
        ));
    }
    if summary_only && trace.is_none() {
        return Err(DevtoolError::Usage(
            "trace-summary requires --trace <file>".into(),
        ));
    }
    if !explicit_hz {
        println!("refresh rate assumed: 60 Hz; set --refresh-hz or make HZ=<rate>");
    }
    Ok((options, trace))
}

/// Exports event timestamps and zone aggregates with Tracy's official CSV exporter.
fn export(trace: &Path, options: &Options) -> Result<Summary, DevtoolError> {
    let frames = csv_export(
        trace,
        &["-u", "-f", &options.frame_zone],
        &options.output.join("frames.csv"),
    )?;
    let zones = csv_export(trace, &[], &options.output.join("zones.csv"))?;
    summarize(&frames, &zones, options.refresh_hz, &options.frame_zone)
}

/// Writes an export as a local artifact and reports incompatible traces as errors.
fn csv_export(trace: &Path, args: &[&str], path: &Path) -> Result<String, DevtoolError> {
    let output = Command::new("tracy-csvexport")
        .args(args)
        .arg(trace)
        .output()
        .map_err(io_error)?;
    if !output.status.success() {
        return Err(DevtoolError::Usage(format!(
            "Tracy export failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    fs::write(path, &output.stdout).map_err(io_error)?;
    String::from_utf8(output.stdout)
        .map_err(|_| DevtoolError::Usage("Tracy export is not UTF-8".into()))
}

/// Measures intervals between presentation starts, rather than the duration of a CPU zone.
fn summarize(
    frames: &str,
    zones: &str,
    refresh_hz: f64,
    frame_zone: &str,
) -> Result<Summary, DevtoolError> {
    let mut reader = csv::Reader::from_reader(frames.as_bytes());
    let headers = reader.headers().map_err(csv_error)?.clone();
    let name = column(&headers, "name")?;
    let thread = column(&headers, "thread")?;
    let start = column(&headers, "ns_since_start")?;
    let mut by_thread = std::collections::BTreeMap::<String, Vec<f64>>::new();
    for record in reader.records() {
        let record = record.map_err(csv_error)?;
        if &record[name] == frame_zone {
            by_thread
                .entry(record[thread].into())
                .or_default()
                .push(number(&record[start])?);
        }
    }
    if by_thread.len() != 1 {
        return Err(DevtoolError::Usage(format!(
            "expected one presentation thread for {frame_zone}, found {}",
            by_thread.len()
        )));
    }
    let mut timestamps = by_thread.into_values().next().unwrap();
    timestamps.sort_by(f64::total_cmp);
    let mut intervals: Vec<_> = timestamps
        .windows(2)
        .map(|pair| (pair[1] - pair[0]) / 1_000_000.0)
        .collect();
    if intervals.is_empty() {
        return Err(DevtoolError::Usage(
            "trace contains fewer than two presentation events".into(),
        ));
    }
    intervals.sort_by(f64::total_cmp);
    let mut reader = csv::Reader::from_reader(zones.as_bytes());
    let headers = reader.headers().map_err(csv_error)?.clone();
    let name = column(&headers, "name")?;
    let total = column(&headers, "total_ns")?;
    let maximum = column(&headers, "max_ns")?;
    let source_file = headers.iter().position(|header| header == "src_file");
    let source_line = headers.iter().position(|header| header == "src_line");
    let mut zones = Vec::new();
    for record in reader.records() {
        let record = record.map_err(csv_error)?;
        zones.push(Zone {
            name: record[name].into(),
            source: source_file
                .zip(source_line)
                .map_or_else(String::new, |(file, line)| {
                    format!("{}:{}", &record[file], &record[line])
                }),
            total_ms: number(&record[total])? / 1_000_000.0,
            max_ms: number(&record[maximum])? / 1_000_000.0,
        });
    }
    Ok(Summary {
        refresh_hz,
        frame_zone: frame_zone.into(),
        frames: intervals.len(),
        p50_ms: percentile(&intervals, 0.5),
        p90_ms: percentile(&intervals, 0.9),
        p99_ms: percentile(&intervals, 0.99),
        hitches: intervals
            .iter()
            .filter(|interval| **interval > 1500.0 / refresh_hz)
            .count(),
        zones,
    })
}

/// Selects a nearest-rank percentile from nonempty sorted intervals.
fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    sorted[((sorted.len() as f64 * fraction).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1)]
}

/// Resolves CSV columns by name, tolerating exporter column reordering.
fn column(headers: &csv::StringRecord, name: &str) -> Result<usize, DevtoolError> {
    headers
        .iter()
        .position(|header| header == name)
        .ok_or_else(|| DevtoolError::Usage(format!("Tracy export missing column {name}")))
}

/// Rejects invalid measurements instead of silently interpreting them as zero.
fn number(text: &str) -> Result<f64, DevtoolError> {
    text.parse::<f64>()
        .ok()
        .filter(|number| number.is_finite() && *number >= 0.0)
        .ok_or_else(|| DevtoolError::Usage(format!("invalid Tracy measurement: {text}")))
}

/// Describes exporter CSV failures in the command error type.
fn csv_error(error: csv::Error) -> DevtoolError {
    DevtoolError::Usage(format!("invalid Tracy CSV: {error}"))
}

/// Reports percentiles, deadline misses, and inclusive zones without adding overlapping time.
fn print_summary(summary: &Summary) {
    println!(
        "frames={} refresh_hz={} interval_ms p50={:.3} p90={:.3} p99={:.3} over_1.5_refresh={}",
        summary.frames,
        summary.refresh_hz,
        summary.p50_ms,
        summary.p90_ms,
        summary.p99_ms,
        summary.hitches
    );
    for (label, by_max) in [("total", false), ("max", true)] {
        let mut zones = summary.zones.clone();
        zones.sort_by(|a, b| {
            if by_max {
                b.max_ms.total_cmp(&a.max_ms)
            } else {
                b.total_ms.total_cmp(&a.total_ms)
            }
        });
        println!("top zones by {label} (inclusive ms):");
        for zone in zones.iter().take(10) {
            println!(
                "  {} total={:.3} max={:.3}",
                zone.name, zone.total_ms, zone.max_ms
            );
        }
    }
}

/// Compares summaries only when frame definitions and refresh intervals agree.
fn print_diff(base: &Summary, head: &Summary) -> Result<(), DevtoolError> {
    if base.refresh_hz != head.refresh_hz || base.frame_zone != head.frame_zone {
        return Err(DevtoolError::Usage(
            "baseline refresh rate and frame zone must match".into(),
        ));
    }
    println!(
        "baseline delta_ms p50={:+.3} p90={:+.3} p99={:+.3} hitch_delta={:+} hitch_rate_delta={:+.4}",
        head.p50_ms - base.p50_ms,
        head.p90_ms - base.p90_ms,
        head.p99_ms - base.p99_ms,
        head.hitches as i64 - base.hitches as i64,
        head.hitches as f64 / head.frames as f64 - base.hitches as f64 / base.frames as f64
    );
    for zone in &head.zones {
        if let Some(previous) = base
            .zones
            .iter()
            .find(|previous| previous.name == zone.name && previous.source == zone.source)
        {
            println!(
                "  {} total_ms_delta={:+.3} max_ms_delta={:+.3}",
                zone.name,
                zone.total_ms - previous.total_ms,
                zone.max_ms - previous.max_ms
            );
        }
    }
    Ok(())
}

const CAPTURE_ADDRESS: &str = "127.0.0.1";
const CONNECTION_ALLOWANCE: Duration = Duration::from_secs(30);
const PROCESS_POLL: Duration = Duration::from_millis(20);
const TERMINATION_GRACE: Duration = Duration::from_secs(1);

/// Chooses a free loopback port, releasing it immediately before the client starts listening.
fn capture_port() -> Result<u16, DevtoolError> {
    TcpListener::bind((CAPTURE_ADDRESS, 0))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .map_err(io_error)
}

/// Polls both owned processes, bounding connection wait plus the requested capture duration.
fn wait_for_capture(
    client: &mut OwnedChild,
    collector: &mut OwnedChild,
    limit: Duration,
) -> Result<(), DevtoolError> {
    let deadline = Instant::now() + limit;
    loop {
        let captured = collector.0.try_wait().map_err(io_error)?;
        if let Some(status) = client.0.try_wait().map_err(io_error)?
            && (captured.is_none() || !status.success())
        {
            return Err(DevtoolError::Usage(format!(
                "client exited before Tracy capture completed: {status}"
            )));
        }
        if let Some(status) = captured {
            return if status.success() {
                Ok(())
            } else {
                Err(DevtoolError::Usage(format!(
                    "Tracy collector failed: {status}"
                )))
            };
        }
        if Instant::now() >= deadline {
            return Err(DevtoolError::Usage(
                "Tracy exceeded its connection allowance and capture duration".into(),
            ));
        }
        std::thread::sleep(PROCESS_POLL);
    }
}

struct OwnedChild(Child);
impl OwnedChild {
    /// Stops only this owned process, allowing a brief graceful exit before a forced stop.
    fn stop(&mut self) -> Result<(), DevtoolError> {
        if self.0.try_wait().map_err(io_error)?.is_none() {
            #[cfg(unix)]
            {
                checked(Command::new("kill").args(["-TERM", &self.0.id().to_string()]))?;
            }
            #[cfg(not(unix))]
            self.0.kill().map_err(io_error)?;
            let deadline = Instant::now() + TERMINATION_GRACE;
            while self.0.try_wait().map_err(io_error)?.is_none() {
                if Instant::now() >= deadline {
                    self.0.kill().map_err(io_error)?;
                    self.0.wait().map_err(io_error)?;
                    break;
                }
                std::thread::sleep(PROCESS_POLL);
            }
        }
        Ok(())
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Runs build and capture tools and preserves their failure status.
fn checked(command: &mut Command) -> Result<(), DevtoolError> {
    let status = command.status().map_err(io_error)?;
    if status.success() {
        Ok(())
    } else {
        Err(DevtoolError::Usage(format!(
            "capture command failed: {status}"
        )))
    }
}

/// Wraps file and process failures without hiding their causes.
fn io_error(source: std::io::Error) -> DevtoolError {
    DevtoolError::Spawn {
        command: "performance capture".into(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn waiter_child() {
        if std::env::var_os("CAPTURE_WAIT_FIXTURE").is_some() {
            use std::io::Read;
            std::io::stdin().read_to_end(&mut Vec::new()).unwrap();
        }
    }

    #[test]
    fn collector_deadline_reports_a_connection_failure_and_cleans_owned_processes() {
        use std::process::Stdio;
        let spawn = || {
            OwnedChild(
                Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "capture::tests::waiter_child", "--nocapture"])
                    .env("CAPTURE_WAIT_FIXTURE", "1")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            )
        };
        let mut client = spawn();
        let mut collector = spawn();
        let error = wait_for_capture(&mut client, &mut collector, Duration::ZERO).unwrap_err();
        assert!(error.to_string().contains("connection allowance"));
        client.stop().unwrap();
        collector.stop().unwrap();
        assert!(client.0.try_wait().unwrap().is_some());
        assert!(collector.0.try_wait().unwrap().is_some());
    }

    #[test]
    fn intervals_use_presentation_starts_and_zones_support_quoted_names() {
        let frames = "name,ns_since_start,exec_time_ns,thread\npresent_frames,0,1,1\npresent_frames,10000000,1,1\npresent_frames,20000000,1,1\npresent_frames,50000000,1,1\nother,1,9999999,2\n";
        let zones = "name,total_ns,max_ns\n\"zone, quoted\",4000000,3000000\n";
        let summary = summarize(frames, zones, 60.0, "present_frames").unwrap();
        assert_eq!(
            (summary.p50_ms, summary.p90_ms, summary.p99_ms),
            (10.0, 30.0, 30.0)
        );
        assert_eq!(summary.hitches, 1);
        assert_eq!(summary.zones[0].name, "zone, quoted");
        assert_eq!(summary.zones[0].total_ms, 4.0);
    }
    #[test]
    fn missing_or_ambiguous_frames_and_mismatched_baselines_fail() {
        assert!(summarize("name,thread,ns_since_start\n", "", 60.0, "present_frames").is_err());
        assert!(parse(&["--refresh-hz".into(), "NaN".into()], false).is_err());
    }
}

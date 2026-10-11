use anyhow::{Result, bail, ensure};
use std::{path::PathBuf, time::Duration};

/// Replay ceilings leave room for session framing inside the client's transport limits.
pub const MAX_BURST_BYTES: usize = 8 << 20;
pub const MAX_BURST_PACKETS: usize = 1024;

#[derive(Debug)]
pub struct Options {
    pub capture: PathBuf,
    pub socket_dir: PathBuf,
    pub report: PathBuf,
    pub packs: Vec<PathBuf>,
    pub burst_packets: usize,
    pub burst_bytes: usize,
    pub interval: Duration,
    pub timeout: Duration,
    pub hold: Duration,
}

impl Options {
    /// Accepts the Go command's single/double-dash flags, including flag=value and duration units.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Option<Self>> {
        let mut result = Self {
            capture: PathBuf::new(),
            socket_dir: PathBuf::new(),
            report: PathBuf::new(),
            packs: Vec::new(),
            burst_packets: 32,
            burst_bytes: MAX_BURST_BYTES,
            interval: Duration::from_millis(50),
            timeout: Duration::from_secs(120),
            hold: Duration::ZERO,
        };
        let mut args = args.into_iter();
        while let Some(flag) = args.next() {
            ensure!(
                flag.starts_with('-'),
                "positional arguments are not supported"
            );
            let flag = flag
                .strip_prefix("--")
                .or_else(|| flag.strip_prefix('-'))
                .unwrap();
            if matches!(flag, "h" | "help") {
                return Ok(None);
            }
            let (name, value) = match flag.split_once('=') {
                Some((name, value)) => (name, value.to_owned()),
                None => (
                    flag,
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("missing value for {flag}"))?,
                ),
            };
            match name {
                "capture" => result.capture = value.into(),
                "socket-dir" => result.socket_dir = value.into(),
                "report" => result.report = value.into(),
                "resource-pack" => result.packs.push(value.into()),
                "burst-packets" => result.burst_packets = value.parse()?,
                "burst-bytes" => result.burst_bytes = value.parse()?,
                "burst-interval" => result.interval = parse_duration(&value)?,
                "timeout" => result.timeout = parse_duration(&value)?,
                "hold" => result.hold = parse_duration(&value)?,
                _ => bail!("unknown flag: {name}"),
            }
        }
        ensure!(
            !result.capture.as_os_str().is_empty()
                && !result.socket_dir.as_os_str().is_empty()
                && !result.report.as_os_str().is_empty(),
            "capture, socket-dir and report are required"
        );
        ensure!(
            (1..=MAX_BURST_PACKETS).contains(&result.burst_packets)
                && (1..=MAX_BURST_BYTES).contains(&result.burst_bytes)
                && !result.timeout.is_zero(),
            "invalid replay bounds"
        );
        Ok(Some(result))
    }
}

/// Parses Go-style compound durations, bounded by Go's signed nanosecond duration range.
fn parse_duration(text: &str) -> Result<Duration> {
    let negative = text.starts_with('-');
    let text = text.strip_prefix(['+', '-']).unwrap_or(text);
    if text == "0" {
        return Ok(Duration::ZERO);
    }
    let mut rest = text;
    let mut nanos = 0u128;
    while !rest.is_empty() {
        let length = rest
            .find(|c: char| !c.is_ascii_digit() && c != '.')
            .unwrap_or(rest.len());
        let number = &rest[..length];
        ensure!(!number.is_empty(), "invalid duration: {text}");
        rest = &rest[length..];
        let (unit, scale) = [
            ("ns", 1u128),
            ("us", 1_000),
            ("µs", 1_000),
            ("μs", 1_000),
            ("ms", 1_000_000),
            ("s", 1_000_000_000),
            ("m", 60_000_000_000),
            ("h", 3_600_000_000_000),
        ]
        .into_iter()
        .find(|(unit, _)| rest.starts_with(unit))
        .ok_or_else(|| anyhow::anyhow!("invalid duration unit: {text}"))?;
        rest = &rest[unit.len()..];
        let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
        ensure!(
            !whole.is_empty() || !fraction.is_empty(),
            "invalid duration: {text}"
        );
        let whole = if whole.is_empty() {
            0
        } else {
            whole.parse::<u128>()?
        };
        ensure!(
            fraction.bytes().all(|b| b.is_ascii_digit()),
            "invalid duration: {text}"
        );
        // Extra precision below a nanosecond is discarded, as with Go duration flags.
        let fraction = &fraction[..fraction.len().min(18)];
        let fractional = if fraction.is_empty() {
            0
        } else {
            fraction.parse::<u128>()? * scale / 10u128.pow(fraction.len() as u32)
        };
        nanos = nanos
            .checked_add(
                whole
                    .checked_mul(scale)
                    .ok_or_else(|| anyhow::anyhow!("duration overflow"))?,
            )
            .and_then(|n| n.checked_add(fractional))
            .ok_or_else(|| anyhow::anyhow!("duration overflow"))?;
        ensure!(nanos <= i64::MAX as u128, "duration overflow");
    }
    ensure!(!text.is_empty(), "empty duration");
    ensure!(!negative || nanos == 0, "negative duration");
    Ok(Duration::from_nanos(nanos as u64))
}

pub const HELP: &str = "bedrock-replay -capture raw.bin -socket-dir DIR -report report.json\n  -resource-pack FILE (repeat in stack order)\n  -burst-packets 32 -burst-bytes 8388608 -burst-interval 50ms\n  -timeout 2m -hold 0s\n";

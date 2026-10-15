//! What a crash folder carries from the client's staging directory beside its fixed members:
//! [`folder::PACKETS_FILE`], the packet header ring as a table with packet names;
//! [`folder::DECODE_SAMPLES_DIR`], the rejected packet samples as the client saved them; and
//! [`folder::FLIGHT_LOG_FILE`], the flight recorder's last log lines. Each is read from files
//! the client keeps current while it runs, so they are complete after any crash.

use std::{fmt::Write as _, fs, path::Path};

use crash_report::{
    folder,
    ring::{
        flight,
        packets::{self, PacketImage},
    },
    staging,
};
use protocol::packet_tap::{PacketDirection, PacketName, PacketOutcome};

/// Packet rows the crash window lists, newest last.
pub const WINDOW_PACKETS: usize = 40;
/// Largest sample file copied into a report; the client bounds a session's samples to less.
const MAX_SAMPLE_FILE_BYTES: u64 = 2 << 20;
/// Column headings of the [`folder::PACKETS_FILE`] table.
const PACKETS_HEADING: &str = "  sequence  elapsed_ms  direction    outcome       bytes  packet";

/// The files a report attaches, read from a staging directory.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Attachments {
    files: Vec<(String, Vec<u8>)>,
}

impl Attachments {
    /// Reads the staging directory `staging_dir`; a missing or unreadable ring or sample leaves
    /// its attachment out rather than failing the report.
    pub fn read(staging_dir: &Path) -> Self {
        let mut files = Vec::new();
        if let Some(image) = fs::read(staging_dir.join(staging::PACKET_RING_FILE))
            .ok()
            .and_then(|bytes| packets::read(&bytes).ok())
        {
            files.push((
                folder::PACKETS_FILE.to_owned(),
                packets_table(&image).into_bytes(),
            ));
        }
        let samples = staging_dir.join(staging::DECODE_SAMPLES_DIR);
        let mut sample_files: Vec<(String, Vec<u8>)> = fs::read_dir(&samples)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| {
                entry.file_type().is_ok_and(|kind| kind.is_file())
                    && entry
                        .metadata()
                        .is_ok_and(|metadata| metadata.len() <= MAX_SAMPLE_FILE_BYTES)
            })
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let bytes = fs::read(entry.path()).ok()?;
                Some((format!("{}/{name}", folder::DECODE_SAMPLES_DIR), bytes))
            })
            .collect();
        sample_files.sort();
        files.extend(sample_files);
        if let Some(image) = fs::read(staging_dir.join(staging::FLIGHT_RING_FILE))
            .ok()
            .and_then(|bytes| flight::read(&bytes).ok())
        {
            files.push((
                folder::FLIGHT_LOG_FILE.to_owned(),
                image.to_log().into_bytes(),
            ));
        }
        Self { files }
    }

    /// The attachments as [`folder::write_with`] takes them.
    pub fn as_folder(&self) -> Vec<folder::Attachment<'_>> {
        self.files
            .iter()
            .map(|(name, bytes)| folder::Attachment { name, bytes })
            .collect()
    }

    /// Names of the attached files, in write order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(|(name, _)| name.as_str())
    }
}

/// The [`folder::PACKETS_FILE`] text: a summary line, the headings, then one row per retained
/// header, oldest first.
fn packets_table(image: &PacketImage) -> String {
    let mut table = format!(
        "packet headers: {} recorded, last {} kept\n{PACKETS_HEADING}\n",
        image.recorded,
        image.headers.len()
    );
    for header in &image.headers {
        let direction =
            PacketDirection::from_wire(header.direction).map_or("unknown", |d| d.name());
        let outcome = PacketOutcome::from_wire(header.outcome).map_or("unknown", |o| o.name());
        let _ = writeln!(
            table,
            "{:>10}  {:>10}  {direction:<11}  {outcome:<8}  {:>9}  {} ({})",
            header.sequence,
            header.elapsed_ms,
            header.len,
            PacketName(header.packet_id),
            header.packet_id,
        );
    }
    table
}

/// The last `count` rows of a [`folder::PACKETS_FILE`] table as the crash window lists them,
/// oldest first: `+<seconds>s  in|out  <name>  <bytes> B`, with the outcome unless accepted.
/// Lines that are not table rows are skipped.
pub fn window_lines(table: &str, count: usize) -> Vec<String> {
    let rows: Vec<String> = table.lines().filter_map(window_line).collect();
    rows[rows.len().saturating_sub(count)..].to_vec()
}

fn window_line(row: &str) -> Option<String> {
    let mut columns = row.split_whitespace();
    let _sequence: u64 = columns.next()?.parse().ok()?;
    let elapsed_ms: u64 = columns.next()?.parse().ok()?;
    let direction = match columns.next()? {
        "clientbound" => "in ",
        "serverbound" => "out",
        _ => "?  ",
    };
    let outcome = columns.next()?;
    let bytes: u64 = columns.next()?.parse().ok()?;
    let name = columns.next()?;
    let mut line = format!(
        "+{}.{:03}s  {direction}  {name}  {bytes} B",
        elapsed_ms / 1000,
        elapsed_ms % 1000
    );
    if outcome != PacketOutcome::Accepted.name() {
        let _ = write!(line, "  {outcome}");
    }
    Some(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crash_report::ring::{flight::FlightRing, packets::PacketRing};

    fn start_game() -> u32 {
        (0..0x400)
            .find(|id| PacketName(*id).to_string() == "StartGame")
            .expect("the pinned protocol has StartGame")
    }

    #[test]
    fn a_staging_directory_becomes_named_packets_samples_and_the_flight_log() {
        let staging_dir = tempfile::tempdir().unwrap();
        let ring = Box::new(PacketRing::new());
        ring.attach(&staging_dir.path().join(staging::PACKET_RING_FILE))
            .unwrap();
        let start_game = start_game();
        ring.record(
            PacketDirection::Clientbound as u8,
            start_game,
            58,
            11_912,
            PacketOutcome::Accepted as u8,
        );
        ring.record(
            PacketDirection::Serverbound as u8,
            0x3ff,
            4,
            11_940,
            PacketOutcome::Failed as u8,
        );
        static FLIGHT: FlightRing = FlightRing::new();
        FLIGHT
            .attach(&staging_dir.path().join(staging::FLIGHT_RING_FILE))
            .unwrap();
        FLIGHT.record(1_760_443_200_123, b"DEBUG main protocol: decoded StartGame");
        let samples = staging_dir.path().join(staging::DECODE_SAMPLES_DIR);
        fs::create_dir_all(&samples).unwrap();
        fs::write(samples.join("index.txt"), b"index").unwrap();
        fs::write(samples.join("2-Unknown(1023).bin"), [1, 2]).unwrap();

        let attachments = Attachments::read(staging_dir.path());
        assert_eq!(
            attachments.names().collect::<Vec<_>>(),
            [
                folder::PACKETS_FILE,
                "decode-samples/2-Unknown(1023).bin",
                "decode-samples/index.txt",
                folder::FLIGHT_LOG_FILE,
            ]
        );
        let table = String::from_utf8(attachments.files[0].1.clone()).unwrap();
        let lines: Vec<_> = table.lines().collect();
        assert_eq!(lines[0], "packet headers: 2 recorded, last 2 kept");
        assert_eq!(lines[1], PACKETS_HEADING);
        assert!(
            lines[2].ends_with(&format!("StartGame ({start_game})")),
            "{table}"
        );
        assert!(
            lines[3].contains("serverbound") && lines[3].contains("failed"),
            "{table}"
        );
        assert_eq!(
            window_lines(&table, WINDOW_PACKETS),
            [
                "+11.912s  in   StartGame  58 B",
                "+11.940s  out  Unknown(1023)  4 B  failed",
            ]
        );
        assert_eq!(window_lines(&table, 1).len(), 1);
        let flight = String::from_utf8(attachments.files[3].1.clone()).unwrap();
        assert_eq!(
            flight,
            "flight recorder: 1 lines recorded, last 1 kept\n\
             2025-10-14T12:00:00.123Z  DEBUG main protocol: decoded StartGame\n"
        );
    }

    #[test]
    fn an_empty_or_foreign_staging_directory_attaches_nothing() {
        let staging_dir = tempfile::tempdir().unwrap();
        assert!(Attachments::read(staging_dir.path()).as_folder().is_empty());
        fs::write(staging_dir.path().join(staging::PACKET_RING_FILE), b"junk").unwrap();
        assert!(
            Attachments::read(&staging_dir.path().join("missing"))
                .as_folder()
                .is_empty()
        );
        assert!(Attachments::read(staging_dir.path()).as_folder().is_empty());
    }
}

//! Protocol capture: records what the real game client sends and receives so the
//! undocumented parts of its protocols can be pinned down and turned into test
//! fixtures.
//!
//! Each run writes to `<capture dir>/<UTC start time>/`:
//! - `http.jsonl`: one JSON object per request/response pair (bodies uncompressed)
//! - `xmpp.log`: raw stream bytes per connection, as text when printable
//! - `udp-<server>.pcap` (raw IPv4, opens in Wireshark) and `udp-<server>.hexlog`
//!
//! Recording goes through a channel to a writer thread, so callers never block
//! on disk I/O. Captures contain session tokens; redact before sharing.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::mpsc;
use std::thread;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureStream {
    Http,
    Xmpp,
    UdpFreeroam,
    UdpRace,
}

impl CaptureStream {
    pub fn name(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Xmpp => "xmpp",
            Self::UdpFreeroam => "udp-freeroam",
            Self::UdpRace => "udp-race",
        }
    }
}

impl FromStr for CaptureStream {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        [Self::Http, Self::Xmpp, Self::UdpFreeroam, Self::UdpRace]
            .into_iter()
            .find(|stream| stream.name() == s)
            .ok_or(())
    }
}

/// Which way a packet travelled, from the server's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// Client to server.
    In,
    /// Server to client.
    Out,
}

impl Direction {
    fn label(self) -> &'static str {
        match self {
            Self::In => "IN ",
            Self::Out => "OUT",
        }
    }
}

/// One HTTP exchange. Bodies are recorded before compression.
#[derive(Debug, Clone, Serialize)]
pub struct HttpExchange {
    pub method: String,
    pub uri: String,
    pub request_headers: Vec<(String, String)>,
    pub request_body: String,
    pub status: u16,
    pub response_headers: Vec<(String, String)>,
    pub response_body: String,
}

enum Record {
    Http(OffsetDateTime, HttpExchange),
    Xmpp { at: OffsetDateTime, conn: u64, dir: Direction, peer: SocketAddr, bytes: Vec<u8> },
    Udp { at: OffsetDateTime, stream: CaptureStream, dir: Direction, src: SocketAddr, dst: SocketAddr, bytes: Vec<u8> },
}

/// Cheap to clone; a disabled sink drops everything.
#[derive(Clone, Default)]
pub struct CaptureSink {
    tx: Option<mpsc::Sender<Record>>,
    streams: Vec<CaptureStream>,
    run_dir: Option<PathBuf>,
}

impl CaptureSink {
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Start recording `streams` into a new timestamped directory under `dir`.
    /// Returns a disabled sink when `streams` is empty.
    pub fn start(dir: &Path, streams: &[CaptureStream]) -> io::Result<Self> {
        if streams.is_empty() {
            return Ok(Self::disabled());
        }
        let stamp = OffsetDateTime::now_utc()
            .format(format_description!("[year][month][day]T[hour][minute][second]Z"))
            .expect("static format");
        let run_dir = dir.join(stamp);
        fs::create_dir_all(&run_dir)?;
        let (tx, rx) = mpsc::channel();
        let writer = Writer::new(run_dir.clone());
        thread::Builder::new().name("capture-writer".into()).spawn(move || writer.run(rx))?;
        Ok(Self { tx: Some(tx), streams: streams.to_vec(), run_dir: Some(run_dir) })
    }

    pub fn enabled(&self, stream: CaptureStream) -> bool {
        self.tx.is_some() && self.streams.contains(&stream)
    }

    /// The directory this run writes to, when capture is on.
    pub fn run_dir(&self) -> Option<&Path> {
        self.run_dir.as_deref()
    }

    pub fn http(&self, exchange: HttpExchange) {
        if self.enabled(CaptureStream::Http) {
            self.send(Record::Http(OffsetDateTime::now_utc(), exchange));
        }
    }

    pub fn xmpp(&self, conn: u64, dir: Direction, peer: SocketAddr, bytes: &[u8]) {
        if self.enabled(CaptureStream::Xmpp) {
            let at = OffsetDateTime::now_utc();
            self.send(Record::Xmpp { at, conn, dir, peer, bytes: bytes.to_vec() });
        }
    }

    /// `src` and `dst` are the real endpoints of the datagram, in travel order.
    pub fn udp(&self, stream: CaptureStream, dir: Direction, src: SocketAddr, dst: SocketAddr, bytes: &[u8]) {
        if self.enabled(stream) {
            let at = OffsetDateTime::now_utc();
            self.send(Record::Udp { at, stream, dir, src, dst, bytes: bytes.to_vec() });
        }
    }

    fn send(&self, record: Record) {
        if let Some(tx) = &self.tx {
            // The writer only goes away if it hit an I/O error, which it logged.
            let _ = tx.send(record);
        }
    }
}

struct Writer {
    dir: PathBuf,
    files: HashMap<String, BufWriter<File>>,
}

impl Writer {
    fn new(dir: PathBuf) -> Self {
        Self { dir, files: HashMap::new() }
    }

    fn run(mut self, rx: mpsc::Receiver<Record>) {
        for record in rx {
            if let Err(err) = self.write(record) {
                tracing::error!(%err, dir = %self.dir.display(), "capture writer failed; capture stopped");
                return;
            }
        }
    }

    fn write(&mut self, record: Record) -> io::Result<()> {
        match record {
            Record::Http(at, exchange) => {
                let mut line = serde_json::to_value(&exchange)?;
                line["at"] = rfc3339(at).into();
                let file = self.file("http.jsonl", |_| Ok(()))?;
                serde_json::to_writer(&mut *file, &line)?;
                file.write_all(b"\n")?;
                file.flush()
            }
            Record::Xmpp { at, conn, dir, peer, bytes } => {
                let file = self.file("xmpp.log", |_| Ok(()))?;
                writeln!(file, "{} conn={conn} {} {peer} len={}", rfc3339(at), dir.label(), bytes.len())?;
                match std::str::from_utf8(&bytes) {
                    Ok(text) if text.chars().all(|c| !c.is_control() || c.is_whitespace()) => {
                        writeln!(file, "{text}")?;
                    }
                    _ => file.write_all(hexdump(&bytes).as_bytes())?,
                }
                writeln!(file)?;
                file.flush()
            }
            Record::Udp { at, stream, dir, src, dst, bytes } => {
                let hexlog = self.file(&format!("{}.hexlog", stream.name()), |_| Ok(()))?;
                writeln!(hexlog, "{} {} {src} -> {dst} len={}", rfc3339(at), dir.label(), bytes.len())?;
                hexlog.write_all(hexdump(&bytes).as_bytes())?;
                writeln!(hexlog)?;
                hexlog.flush()?;
                if let (SocketAddr::V4(src), SocketAddr::V4(dst)) = (src, dst) {
                    let pcap = self.file(&format!("{}.pcap", stream.name()), pcap::write_header)?;
                    pcap::write_udp_record(pcap, at, src, dst, &bytes)?;
                    pcap.flush()?;
                }
                Ok(())
            }
        }
    }

    fn file(
        &mut self,
        name: &str,
        init: impl FnOnce(&mut BufWriter<File>) -> io::Result<()>,
    ) -> io::Result<&mut BufWriter<File>> {
        if !self.files.contains_key(name) {
            let mut file = BufWriter::new(File::create(self.dir.join(name))?);
            init(&mut file)?;
            self.files.insert(name.to_owned(), file);
        }
        Ok(self.files.get_mut(name).expect("inserted above"))
    }
}

fn rfc3339(at: OffsetDateTime) -> String {
    at.format(&Rfc3339).expect("UTC timestamps always format")
}

/// Classic 16-bytes-per-line hex dump with an ASCII column.
pub fn hexdump(bytes: &[u8]) -> String {
    let mut out = String::new();
    for (row, chunk) in bytes.chunks(16).enumerate() {
        let _ = write!(out, "{:04x}  ", row * 16);
        for i in 0..16 {
            match chunk.get(i) {
                Some(b) => {
                    let _ = write!(out, "{b:02x} ");
                }
                None => out.push_str("   "),
            }
            if i == 7 {
                out.push(' ');
            }
        }
        out.push_str(" |");
        out.extend(chunk.iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' }));
        out.push_str("|\n");
    }
    out
}

/// Minimal pcap writer: raw IPv4 link type with synthesised IP/UDP headers, so
/// captures open in Wireshark with the real addresses and ports.
mod pcap {
    use std::io::{self, Write};
    use std::net::SocketAddrV4;

    use time::OffsetDateTime;

    const LINKTYPE_RAW: u32 = 101;

    pub fn write_header(w: &mut impl Write) -> io::Result<()> {
        w.write_all(&0xa1b2_c3d4u32.to_le_bytes())?;
        w.write_all(&2u16.to_le_bytes())?;
        w.write_all(&4u16.to_le_bytes())?;
        w.write_all(&0i32.to_le_bytes())?; // thiszone
        w.write_all(&0u32.to_le_bytes())?; // sigfigs
        w.write_all(&65_535u32.to_le_bytes())?; // snaplen
        w.write_all(&LINKTYPE_RAW.to_le_bytes())
    }

    pub fn write_udp_record(
        w: &mut impl Write,
        at: OffsetDateTime,
        src: SocketAddrV4,
        dst: SocketAddrV4,
        payload: &[u8],
    ) -> io::Result<()> {
        let packet = udp_ipv4_packet(src, dst, payload);
        let len = packet.len() as u32;
        w.write_all(&(at.unix_timestamp() as u32).to_le_bytes())?;
        w.write_all(&(at.microsecond()).to_le_bytes())?;
        w.write_all(&len.to_le_bytes())?;
        w.write_all(&len.to_le_bytes())?;
        w.write_all(&packet)
    }

    pub(super) fn udp_ipv4_packet(src: SocketAddrV4, dst: SocketAddrV4, payload: &[u8]) -> Vec<u8> {
        // Datagrams this large never occur in practice; truncate rather than wrap.
        let payload = &payload[..payload.len().min(65_535 - 28)];
        let total = (20 + 8 + payload.len()) as u16;
        let mut ip = [0u8; 20];
        ip[0] = 0x45; // IPv4, 5-word header
        ip[2..4].copy_from_slice(&total.to_be_bytes());
        ip[8] = 64; // TTL
        ip[9] = 17; // UDP
        ip[12..16].copy_from_slice(&src.ip().octets());
        ip[16..20].copy_from_slice(&dst.ip().octets());
        let checksum = ipv4_checksum(&ip);
        ip[10..12].copy_from_slice(&checksum.to_be_bytes());

        let mut packet = Vec::with_capacity(total as usize);
        packet.extend_from_slice(&ip);
        packet.extend_from_slice(&src.port().to_be_bytes());
        packet.extend_from_slice(&dst.port().to_be_bytes());
        packet.extend_from_slice(&(total - 20).to_be_bytes());
        packet.extend_from_slice(&[0, 0]); // UDP checksum is optional over IPv4
        packet.extend_from_slice(payload);
        packet
    }

    fn ipv4_checksum(header: &[u8; 20]) -> u16 {
        let mut sum: u32 = header.chunks(2).map(|w| u32::from(u16::from_be_bytes([w[0], w[1]]))).sum();
        while sum > 0xffff {
            sum = (sum & 0xffff) + (sum >> 16);
        }
        !(sum as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddrV4};

    #[test]
    fn stream_names_round_trip() {
        for stream in [CaptureStream::Http, CaptureStream::Xmpp, CaptureStream::UdpFreeroam, CaptureStream::UdpRace] {
            assert_eq!(stream.name().parse::<CaptureStream>(), Ok(stream));
        }
        assert!("smtp".parse::<CaptureStream>().is_err());
    }

    #[test]
    fn hexdump_layout() {
        let dump = hexdump(b"<stream:stream to='127.0.0.1'>\x00\x01");
        assert_eq!(
            dump,
            "0000  3c 73 74 72 65 61 6d 3a  73 74 72 65 61 6d 20 74  |<stream:stream t|\n\
             0010  6f 3d 27 31 32 37 2e 30  2e 30 2e 31 27 3e 00 01  |o='127.0.0.1'>..|\n"
        );
    }

    #[test]
    fn ipv4_udp_packet_has_valid_header_checksum() {
        let src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 65, 1), 50_123);
        let dst = SocketAddrV4::new(Ipv4Addr::new(172, 17, 0, 2), 9999);
        let packet = pcap::udp_ipv4_packet(src, dst, &[1, 2, 3]);
        assert_eq!(packet.len(), 31);
        // Re-summing a header that includes its checksum yields 0xffff.
        let sum: u32 = packet[..20].chunks(2).map(|w| u32::from(u16::from_be_bytes([w[0], w[1]]))).sum();
        let folded = (sum & 0xffff) + (sum >> 16);
        assert_eq!(folded, 0xffff);
        assert_eq!(&packet[20..22], &50_123u16.to_be_bytes());
        assert_eq!(&packet[22..24], &9999u16.to_be_bytes());
        assert_eq!(&packet[24..26], &11u16.to_be_bytes());
    }

    #[test]
    fn disabled_sink_records_nothing() {
        let sink = CaptureSink::start(Path::new("/nonexistent"), &[]).unwrap();
        assert!(!sink.enabled(CaptureStream::Http));
        assert!(sink.run_dir().is_none());
    }

    #[test]
    fn writes_all_capture_files() {
        let dir = tempfile::tempdir().unwrap();
        let sink = CaptureSink::start(dir.path(), &[CaptureStream::Http, CaptureStream::Xmpp, CaptureStream::UdpRace])
            .unwrap();
        let run_dir = sink.run_dir().unwrap().to_path_buf();
        let client: SocketAddr = "10.0.0.2:5000".parse().unwrap();
        let server: SocketAddr = "10.0.0.1:9998".parse().unwrap();

        sink.http(HttpExchange {
            method: "GET".into(),
            uri: "/Engine.svc/GetServerInformation".into(),
            request_headers: vec![],
            request_body: String::new(),
            status: 200,
            response_headers: vec![("content-type".into(), "application/json".into())],
            response_body: "{}".into(),
        });
        sink.xmpp(1, Direction::In, client, b"<stream:stream>");
        sink.udp(CaptureStream::UdpRace, Direction::In, client, server, &[0, 1, 2]);
        // Not enabled, so no file appears.
        sink.udp(CaptureStream::UdpFreeroam, Direction::In, client, server, &[0]);
        drop(sink);

        // Records are written in order, so once the last one is on disk all are.
        let pcap_len = 24 + 16 + 31;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while fs::metadata(run_dir.join("udp-race.pcap")).map_or(0, |m| m.len()) < pcap_len
            && std::time::Instant::now() < deadline
        {
            thread::sleep(std::time::Duration::from_millis(10));
        }

        let http = fs::read_to_string(run_dir.join("http.jsonl")).unwrap();
        let line: serde_json::Value = serde_json::from_str(http.lines().next().unwrap()).unwrap();
        assert_eq!(line["uri"], "/Engine.svc/GetServerInformation");
        assert!(line["at"].is_string());
        assert!(fs::read_to_string(run_dir.join("xmpp.log")).unwrap().contains("<stream:stream>"));
        let pcap = fs::read(run_dir.join("udp-race.pcap")).unwrap();
        assert_eq!(pcap.len() as u64, pcap_len);
        assert!(fs::read_to_string(run_dir.join("udp-race.hexlog")).unwrap().contains("00 01 02"));
        assert!(!run_dir.join("udp-freeroam.hexlog").exists());
    }
}

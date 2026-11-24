//! The MCHOSE feature-report protocol (L7 / M7 / A7 / K7 family).
//!
//! Frame: payload[0] is the command, the whole payload is XORed with 0xFF,
//! padding included, and sent as feature report 0x11 (20 bytes) or 0x12
//! (64 bytes). A reply echoes the inverted command in byte 1 and inverts the
//! payload back. There is no checksum.

use crate::hid::HidRaw;
use std::io;
use std::sync::Mutex;
use std::thread::sleep;
use std::time::{Duration, Instant};

pub const REPORT_SHORT: u8 = 0x11;
pub const REPORT_LONG: u8 = 0x12;
pub const DPI_STAGES: usize = 6;

const MIN_GAP: Duration = Duration::from_millis(25);

static LAST: Mutex<Option<Instant>> = Mutex::new(None);
static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn pace() {
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(t) = *last {
        let since = t.elapsed();
        if since < MIN_GAP {
            sleep(MIN_GAP - since);
        }
    }
    *last = Some(Instant::now());
}

pub fn log_line(line: String) {
    let mut log = LOG.lock().unwrap_or_else(|e| e.into_inner());
    log.push(line);
    if log.len() > 240 {
        let extra = log.len() - 240;
        log.drain(0..extra);
    }
}

pub fn log_snapshot() -> Vec<String> {
    LOG.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn payload_len(report: u8) -> usize {
    match report {
        REPORT_SHORT => 20,
        REPORT_LONG => 64,
        _ => 64,
    }
}

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&format!("{b:02x}"));
    }
    s
}

pub fn send(dev: &HidRaw, report: u8, body: &[u8]) -> io::Result<()> {
    let len = payload_len(report);
    if body.len() > len {
        return Err(invalid("command is longer than its report"));
    }
    let mut buf = vec![0xFFu8; len + 1];
    buf[0] = report;
    for (i, b) in body.iter().enumerate() {
        buf[1 + i] = b ^ 0xFF;
    }
    pace();
    let result = dev.set_feature(&buf);
    match &result {
        Ok(()) => log_line(format!("-> {}  {}", hex(&[report]), hex(body))),
        Err(e) => log_line(format!("-> {}  {}  FAILED: {e}", hex(&[report]), hex(body))),
    }
    result
}

pub fn request(dev: &HidRaw, report: u8, body: &[u8]) -> io::Result<Vec<u8>> {
    let len = payload_len(report);
    let command = *body.first().ok_or_else(|| invalid("empty command"))?;
    let mut last = Vec::new();
    for attempt in 0..6u64 {
        send(dev, report, body)?;
        sleep(Duration::from_millis(20));
        let mut buf = vec![0u8; len + 1];
        buf[0] = report;
        pace();
        let n = dev.get_feature(&mut buf)?.max(2).min(buf.len());
        last = buf[..n].to_vec();
        if n >= 3 && (buf[1] ^ 0xFF) == command && buf[2..n].iter().any(|b| *b != 0) {
            let payload: Vec<u8> = buf[2..n].iter().map(|b| b ^ 0xFF).collect();
            log_line(format!(
                "<- {}  {}  {}",
                hex(&[report]),
                hex(&[command]),
                hex(&payload)
            ));
            return Ok(payload);
        }
        log_line(format!(
            "<- {}  no match on try {}: {}",
            hex(&[report]),
            attempt + 1,
            hex(&last)
        ));
        sleep(Duration::from_millis(30 * (attempt + 1)));
    }
    Err(invalid(format!(
        "no reply to command 0x{command:02x} on report 0x{report:02x} (last: {})",
        hex(&last)
    )))
}

#[derive(Clone, Copy, Debug)]
pub struct Identity {
    pub vid: u16,
    pub pid: u16,
    pub firmware: u32,
    pub connect_mode: u8,
    pub connected: bool,
    pub battery: u8,
    pub charging: u8,
}

fn u16le(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn put_u16le(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

/// Read `0x11 0x06`. The first request after idle can come back with a zero
/// vendor ID, so retry instead of reporting a ghost mouse.
pub fn identity(dev: &HidRaw) -> io::Result<Identity> {
    for _ in 0..4 {
        let d = request(dev, REPORT_SHORT, &[0x06])?;
        if d.len() < 11 {
            return Err(invalid("short identity reply"));
        }
        if u16le(&d, 0) == 0 {
            sleep(Duration::from_millis(120));
            continue;
        }
        let flags = d[8];
        return Ok(Identity {
            vid: u16le(&d, 0),
            pid: u16le(&d, 2),
            firmware: u32::from_le_bytes([d[4], d[5], d[6], d[7]]),
            connect_mode: flags & 0b111,
            connected: flags & 0b1000 != 0,
            battery: d[9],
            charging: d[10],
        });
    }
    Err(invalid(
        "the mouse keeps answering an empty identity; reseat the receiver",
    ))
}

/// Read `0x11 0x04`, the firmware version string.
pub fn version_string(dev: &HidRaw) -> io::Result<String> {
    let d = request(dev, REPORT_SHORT, &[0x04])?;
    if d.is_empty() {
        return Ok(String::new());
    }
    let n = (d[0] as usize).min(d.len() - 1);
    Ok(String::from_utf8_lossy(&d[1..1 + n])
        .trim_end_matches('\0')
        .trim()
        .to_string())
}

/// The 63-byte config block shared by read `0x12 0x67` and write `0x12 0x57`.
#[derive(Clone)]
pub struct Config {
    pub body: [u8; 63],
}

const DEVICE_OWNED_BYTE: usize = 3;
const SENSOR_OWNED_BITS: u8 = 0x01;

impl Config {
    pub fn read(dev: &HidRaw) -> io::Result<Self> {
        for attempt in 0..6u64 {
            let d = request(dev, REPORT_LONG, &[0x67])?;
            if d.len() < 63 {
                return Err(invalid("short config reply"));
            }
            if d[..63].iter().any(|b| *b != 0) {
                let mut body = [0u8; 63];
                body.copy_from_slice(&d[..63]);
                return Ok(Self { body });
            }
            if attempt > 0 {
                log_line("config channel answered zeroes; re-selecting profile".into());
                let _ = set_profile(dev, 0);
            }
            sleep(Duration::from_millis(150 * (attempt + 1)));
        }
        Err(invalid(
            "the mouse keeps answering an empty config block; replug the receiver",
        ))
    }

    pub fn write(&self, dev: &HidRaw) -> io::Result<()> {
        let mut frame = Vec::with_capacity(64);
        frame.push(0x57);
        frame.extend_from_slice(&self.body);
        send(dev, REPORT_LONG, &frame)
    }

    pub fn matches(&self, other: &Self) -> bool {
        for (i, (a, b)) in self.body.iter().zip(other.body.iter()).enumerate() {
            if i == DEVICE_OWNED_BYTE {
                continue;
            }
            if i == 17 {
                if a & !SENSOR_OWNED_BITS != b & !SENSOR_OWNED_BITS {
                    return false;
                }
                continue;
            }
            if a != b {
                return false;
            }
        }
        true
    }

    pub fn diff(&self, other: &Self) -> String {
        self.body
            .iter()
            .zip(other.body.iter())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, (a, b))| format!("[{i}] {a:02x}->{b:02x}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn profile(&self) -> u8 {
        self.body[0]
    }

    pub fn wired_rate_index(&self) -> u8 {
        self.body[1] >> 4
    }

    pub fn wired_dpi_stage(&self) -> u8 {
        self.body[1] & 0x0F
    }

    pub fn wireless_rate_index(&self) -> u8 {
        self.body[2] >> 4
    }

    pub fn wireless_dpi_stage(&self) -> u8 {
        self.body[2] & 0x0F
    }

    pub fn set_dpi_stage(&mut self, v: u8) {
        self.body[1] = (self.body[1] & 0xF0) | (v & 0x0F);
        self.body[2] = (self.body[2] & 0xF0) | (v & 0x0F);
    }

    pub fn dpi(&self, stage: usize) -> u16 {
        u16le(&self.body, 4 + stage * 2)
    }

    pub fn dpi_y(&self, stage: usize) -> u16 {
        u16le(&self.body, 51 + stage * 2)
    }

    pub fn set_dpi(&mut self, stage: usize, value: u16) {
        put_u16le(&mut self.body, 4 + stage * 2, value);
        put_u16le(&mut self.body, 51 + stage * 2, value);
    }

    pub fn enabled_stages(&self) -> u8 {
        self.body[16]
    }

    pub fn sensor(&self) -> u8 {
        self.body[17]
    }

    pub fn set_sensor(&mut self, v: u8) {
        self.body[17] = v | (self.body[17] & SENSOR_OWNED_BITS);
    }

    pub fn debounce_ms(&self) -> u8 {
        self.body[18]
    }

    pub fn set_debounce_ms(&mut self, ms: u8) {
        self.body[18] = ms;
    }

    pub fn sleep_minutes(&self) -> u8 {
        self.body[19]
    }

    pub fn set_sleep_minutes(&mut self, minutes: u8) {
        self.body[19] = minutes;
    }

    pub fn rotate(&self) -> u8 {
        self.body[49]
    }

    /// ripple, angle snap, motion sync (bits 2, 3, 4 of the sensor byte).
    pub fn perf_flags(&self) -> (bool, bool, bool) {
        let s = self.body[17];
        (s & 0x04 != 0, s & 0x08 != 0, s & 0x10 != 0)
    }
}

/// `0x11 0x42`: lift-off, ripple, angle snap, motion sync, rotation.
/// Turning a flag off needs the config-block write too, so reconcile.
pub fn set_flags(
    dev: &HidRaw,
    lod: u8,
    ripple: bool,
    angle_snap: bool,
    motion_sync: bool,
    rotate: u8,
) -> io::Result<()> {
    let before = Config::read(dev)?;
    send(
        dev,
        REPORT_SHORT,
        &[
            0x42,
            lod,
            ripple as u8,
            angle_snap as u8,
            motion_sync as u8,
            0,
            0,
            0,
            0,
            rotate,
        ],
    )?;
    sleep(Duration::from_millis(120));

    let wanted = (before.sensor() & !0x1C)
        | ((ripple as u8) << 2)
        | ((angle_snap as u8) << 3)
        | ((motion_sync as u8) << 4);
    let mut after = Config::read(dev)?;
    if after.sensor() == wanted {
        return Ok(());
    }
    after.set_sensor(wanted);
    after.write(dev)?;
    for _ in 0..5 {
        sleep(Duration::from_millis(120));
        if Config::read(dev)?.sensor() == wanted {
            return Ok(());
        }
    }
    Err(invalid("the mouse would not take those sensor flags"))
}

/// `0x11 0x41`: both links in one frame.
pub fn set_report_rate(dev: &HidRaw, wired_index: u8, wireless_index: u8) -> io::Result<()> {
    send(dev, REPORT_SHORT, &[0x41, wired_index, wireless_index])
}

/// `0x11 0x58`.
pub fn set_profile(dev: &HidRaw, profile: u8) -> io::Result<()> {
    send(dev, REPORT_SHORT, &[0x58, profile])
}

/// `0x11 0x02`.
pub fn set_game_mode(dev: &HidRaw, mode: u8) -> io::Result<()> {
    send(dev, REPORT_SHORT, &[0x02, mode])
}

/// Read `0x11 0x03`: bond, vid, pid, link, game mode.
pub fn basics(dev: &HidRaw) -> io::Result<(u16, u16, u8, u8)> {
    let d = request(dev, REPORT_SHORT, &[0x03])?;
    if d.len() < 7 {
        return Err(invalid("short basics reply"));
    }
    Ok((u16le(&d, 1), u16le(&d, 3), d[5], d[6]))
}

/// Poll until the written block reads back, allowing for the bytes the mouse
/// owns itself. A config write needs time to commit.
pub fn confirm(dev: &HidRaw, wrote: &Config) -> io::Result<Config> {
    let mut last = None;
    for attempt in 0..5u64 {
        sleep(Duration::from_millis(80 * (attempt + 1)));
        let after = Config::read(dev)?;
        if after.matches(wrote) {
            return Ok(after);
        }
        last = Some(after);
    }
    let diff = last.as_ref().map(|c| wrote.diff(c)).unwrap_or_default();
    Err(invalid(format!(
        "the mouse did not take the change ({diff})"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_fields_roundtrip() {
        let mut c = Config { body: [0u8; 63] };
        c.set_dpi(2, 1600);
        assert_eq!(c.dpi(2), 1600);
        assert_eq!(c.dpi_y(2), 1600);
        c.set_dpi_stage(4);
        assert_eq!(c.wired_dpi_stage(), 4);
        assert_eq!(c.wireless_dpi_stage(), 4);
        c.set_debounce_ms(7);
        assert_eq!(c.debounce_ms(), 7);
        c.set_sleep_minutes(15);
        assert_eq!(c.sleep_minutes(), 15);
        c.body[17] = 0x01;
        c.set_sensor(0x14);
        assert_eq!(c.perf_flags(), (true, false, true));
        assert_eq!(c.sensor() & 1, 1);
    }

    #[test]
    fn hex_is_spaced() {
        assert_eq!(hex(&[0x11, 0xab, 0x00]), "11 ab 00");
    }
}

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
pub const BUTTON_OFFSET: usize = 20;
pub const BUTTON_SIZE: usize = 4;
/// Entries in the config block. The firmware exposes only the first five for
/// remapping; the sixth is reserved and the DPI button is fixed.
pub const BUTTON_COUNT: usize = 6;

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

    /// The game mode lives in bits 6-7 of the sensor byte: `0b00` is mode 1,
    /// `0b10` is mode 2 and `0b11` is mode 3, matching M HUB.
    pub fn game_mode(&self) -> u8 {
        game_mode_from_sensor(self.body[17])
    }

    pub fn set_game_mode(&mut self, mode: u8) {
        self.body[17] = (self.body[17] & !0xC0) | game_mode_bits(mode);
    }

    /// The LOD index lives in the low two bits of the sensor byte on the
    /// 42 000 DPI models. It is updated by `0x11 0x42` and can be read back.
    pub fn lod_index(&self) -> u8 {
        self.body[17] & 0x03
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

    /// A button entry is four bytes: `(slot << 4) | kind`, then the 24-bit
    /// action value big-endian.
    pub fn button(&self, slot: usize) -> (u8, u32) {
        let at = BUTTON_OFFSET + slot * BUTTON_SIZE;
        let kind = self.body[at] & 0x0f;
        let value = ((self.body[at + 1] as u32) << 16)
            | ((self.body[at + 2] as u32) << 8)
            | self.body[at + 3] as u32;
        (kind, value)
    }

    pub fn set_button(&mut self, slot: usize, kind: u8, value: u32) {
        let at = BUTTON_OFFSET + slot * BUTTON_SIZE;
        self.body[at] = ((slot as u8) << 4) | (kind & 0x0f);
        self.body[at + 1] = (value >> 16) as u8;
        self.body[at + 2] = (value >> 8) as u8;
        self.body[at + 3] = value as u8;
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

/// Map the sensor byte's game-mode bits to the 1-3 number used by the UI.
fn game_mode_from_sensor(sensor: u8) -> u8 {
    match sensor & 0xC0 {
        0x80 => 2,
        0xC0 => 3,
        _ => 1,
    }
}

/// Inverse of [`game_mode_from_sensor`], as the two bits stored in the config
/// block. `0x42` itself takes the 1-3 number directly.
fn game_mode_bits(mode: u8) -> u8 {
    match mode {
        2 => 0x80,
        3 => 0xC0,
        _ => 0x00,
    }
}

/// `0x11 0x42`: lift-off, ripple, angle snap, motion sync, rotation.
/// Turning a flag off needs the config-block write too, so reconcile.
fn reconciled_sensor(
    before: u8,
    after: u8,
    ripple: bool,
    angle_snap: bool,
    motion_sync: bool,
) -> u8 {
    // Bits 0-1 carry the LOD index on the 42 000 DPI models; keep whatever
    // `0x42` just wrote there. Bits 5-7 are the sensor performance mode, which
    // this tool does not manage, so keep them from before the command. Only
    // replace the three flag bits in the middle.
    (before & 0xE0)
        | (after & 0x03)
        | ((ripple as u8) << 2)
        | ((angle_snap as u8) << 3)
        | ((motion_sync as u8) << 4)
}

pub fn set_flags(
    dev: &HidRaw,
    lod: u8,
    ripple: bool,
    angle_snap: bool,
    motion_sync: bool,
    rotate: u8,
) -> io::Result<()> {
    let before = Config::read(dev)?;
    // The `gameMode` argument of `0x42` carries the sensor mode in bits 6-7 of
    // the config byte. Send the current 1-3 value so changing a flag does not
    // silently reset the performance mode.
    let game_mode = before.game_mode();
    let rotate_open = u8::from(rotate != 0);
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
            game_mode,
            rotate_open,
            rotate,
        ],
    )?;
    sleep(Duration::from_millis(120));

    // `0x42` also updates the LOD index in the low two bits of the sensor
    // byte on the 42 000 DPI models. Reconcile the flags against the *new*
    // sensor byte: computing them from the pre-write block and then writing
    // that block back would silently restore the old LOD index.
    let mut after = Config::read(dev)?;
    for _ in 0..4 {
        if after.lod_index() == lod & 0x03 {
            break;
        }
        sleep(Duration::from_millis(60));
        after = Config::read(dev)?;
    }

    let wanted = reconciled_sensor(
        before.sensor(),
        after.sensor(),
        ripple,
        angle_snap,
        motion_sync,
    );
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

/// `0x11 0x42` with the current sensor flags. The game mode is stored in
/// bits 6-7 of the sensor byte and is not readable through `0x11 0x03` on the
/// config node, so it is read back from the config block instead.
pub fn set_game_mode(dev: &HidRaw, mode: u8) -> io::Result<()> {
    if !(1..=3).contains(&mode) {
        return Err(invalid(format!("game mode {mode} out of range")));
    }
    let before = Config::read(dev)?;
    let (ripple, angle_snap, motion_sync) = before.perf_flags();
    let rotate = before.rotate();
    send(
        dev,
        REPORT_SHORT,
        &[
            0x42,
            before.lod_index(),
            ripple as u8,
            angle_snap as u8,
            motion_sync as u8,
            0,
            0,
            mode,
            u8::from(rotate != 0),
            rotate,
        ],
    )?;
    sleep(Duration::from_millis(120));
    for _ in 0..5 {
        if Config::read(dev)?.game_mode() == mode {
            return Ok(());
        }
        sleep(Duration::from_millis(100));
    }
    Err(invalid(format!(
        "the mouse would not enter game mode {mode}"
    )))
}

/// `0x12 0x52`: remap one button. `wire` is the index used by this command,
/// which differs from the config block slot order (see `buttons::BUTTONS`).
/// The next byte is reserved, then the action category and the 24-bit value
/// big-endian.
pub fn set_button(dev: &HidRaw, wire: u8, kind: u8, value: u32) -> io::Result<()> {
    if wire as usize >= BUTTON_COUNT {
        return Err(invalid(format!("button index {wire} out of range")));
    }
    send(
        dev,
        REPORT_LONG,
        &[
            0x52,
            wire,
            0,
            kind & 0x0f,
            (value >> 16) as u8,
            (value >> 8) as u8,
            value as u8,
        ],
    )
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
    fn button_fields_roundtrip() {
        let mut c = Config { body: [0u8; 63] };
        c.set_button(0, 0, 0);
        c.set_button(1, 1, 0x020000);
        c.set_button(5, 10, 0x040000);
        assert_eq!(c.button(0), (0, 0));
        assert_eq!(c.button(1), (1, 0x020000));
        assert_eq!(c.button(5), (10, 0x040000));
        // The high nibble of the type byte identifies the slot, and the
        // value is stored big-endian, as the firmware expects.
        assert_eq!(&c.body[20..24], &[0x00, 0x00, 0x00, 0x00]);
        assert_eq!(&c.body[24..28], &[0x11, 0x02, 0x00, 0x00]);
        assert_eq!(&c.body[40..44], &[0x5a, 0x04, 0x00, 0x00]);
    }

    #[test]
    fn hex_is_spaced() {
        assert_eq!(hex(&[0x11, 0xab, 0x00]), "11 ab 00");
    }

    #[test]
    fn game_mode_uses_the_sensor_high_bits() {
        let mut c = Config { body: [0u8; 63] };
        for (bits, mode) in [(0x00, 1), (0x80, 2), (0xC0, 3)] {
            c.body[17] = bits | 0x1C;
            assert_eq!(c.game_mode(), mode);
        }
        // Bit 6 alone is not a mode the vendor app produces; treat it as 1.
        c.body[17] = 0x40;
        assert_eq!(c.game_mode(), 1);

        c.body[17] = 0x1C;
        c.set_game_mode(2);
        assert_eq!(c.body[17], 0x9C);
        c.set_game_mode(3);
        assert_eq!(c.body[17], 0xDC);
        c.set_game_mode(1);
        assert_eq!(c.body[17], 0x1C);
    }

    #[test]
    fn flag_reconciliation_keeps_the_lod_bits() {
        // before/after: motion-sync off, LOD index 2, performance mode 2.
        let s = reconciled_sensor(0x8e, 0x8e, true, true, false);
        assert_eq!(s & 0x03, 2, "LOD index must survive the flag write");
        assert_eq!(s & 0x1C, 0x0C, "ripple and angle snap are set");
        assert_eq!(s & 0xE0, 0x80, "sensor mode must survive the flag write");
    }

    #[test]
    fn flag_reconciliation_restores_the_mode_even_on_a_bad_readback() {
        // The mouse occasionally reports the mode bits cleared right after
        // 0x42; the config write must put them back rather than accept them.
        let s = reconciled_sensor(0x8e, 0x1e, true, true, true);
        assert_eq!(s & 0xE0, 0x80);
        assert_eq!(s & 0x03, 2);
        assert_eq!(s & 0x1C, 0x1C);
    }
}

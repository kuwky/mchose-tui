//! Raw terminal handling and key decoding, with no external crates.

use std::ffi::{c_int, c_ulong, c_void};
use std::io::{self, IsTerminal, Write};

const STDIN: c_int = 0;
const TCSAFLUSH: c_int = 2;
const TIOCGWINSZ: c_ulong = 0x5413;

const BRKINT: u32 = 0o2;
const ICRNL: u32 = 0o400;
const INPCK: u32 = 0o20;
const ISTRIP: u32 = 0o40;
const IXON: u32 = 0o2000;
const OPOST: u32 = 0o1;
const CS8: u32 = 0o60;
const ECHO: u32 = 0o10;
const ICANON: u32 = 0o2;
const IEXTEN: u32 = 0o100000;
const ISIG: u32 = 0o1;
const VTIME: usize = 5;
const VMIN: usize = 6;

#[repr(C)]
#[derive(Clone, Copy)]
struct Termios {
    c_iflag: u32,
    c_oflag: u32,
    c_cflag: u32,
    c_lflag: u32,
    c_line: u8,
    c_cc: [u8; 32],
    c_ispeed: u32,
    c_ospeed: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

extern "C" {
    fn tcgetattr(fd: c_int, termios_p: *mut Termios) -> c_int;
    fn tcsetattr(fd: c_int, optional_actions: c_int, termios_p: *const Termios) -> c_int;
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
}

pub struct Terminal {
    original: Termios,
    active: bool,
}

impl Terminal {
    pub fn enter() -> io::Result<Self> {
        if !io::stdin().is_terminal() {
            return Err(io::Error::other(
                "stdin is not a terminal; run mchose-tui from a real terminal",
            ));
        }
        let mut t: Termios = unsafe { std::mem::zeroed() };
        if unsafe { tcgetattr(STDIN, &mut t) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let original = t;
        t.c_iflag &= !(BRKINT | ICRNL | INPCK | ISTRIP | IXON);
        t.c_oflag &= !OPOST;
        t.c_cflag |= CS8;
        t.c_lflag &= !(ECHO | ICANON | IEXTEN | ISIG);
        t.c_cc[VMIN] = 0;
        t.c_cc[VTIME] = 1;
        if unsafe { tcsetattr(STDIN, TCSAFLUSH, &t) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut out = io::stdout();
        if let Err(e) = out
            .write_all(b"\x1b[?1049h\x1b[?25l\x1b[2J\x1b[H")
            .and_then(|_| out.flush())
        {
            unsafe {
                tcsetattr(STDIN, TCSAFLUSH, &original);
            }
            return Err(e);
        }
        Ok(Self {
            original,
            active: true,
        })
    }

    pub fn size(&self) -> (usize, usize) {
        let mut ws = Winsize::default();
        let rc = unsafe { ioctl(STDIN, TIOCGWINSZ, &mut ws as *mut Winsize) };
        if rc == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
            (ws.ws_col as usize, ws.ws_row as usize)
        } else {
            (80, 24)
        }
    }

    /// Read whatever is available within ~100 ms. An empty vector means
    /// "no input", which is the redraw tick.
    pub fn keys(&self) -> io::Result<Vec<Key>> {
        let mut buf = [0u8; 64];
        let n = unsafe { read(STDIN, buf.as_mut_ptr() as *mut c_void, buf.len()) };
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() == Some(4) {
                return Ok(Vec::new()); // EINTR
            }
            return Err(e);
        }
        Ok(decode(&buf[..n as usize]))
    }

    pub fn draw(&self, frame: &str) -> io::Result<()> {
        let mut out = io::stdout().lock();
        out.write_all(b"\x1b[H")?;
        out.write_all(frame.as_bytes())?;
        out.write_all(b"\x1b[J")?;
        out.flush()
    }

    fn restore(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        let mut out = io::stdout();
        let _ = out.write_all(b"\x1b[0m\x1b[?25h\x1b[?1049l");
        let _ = out.flush();
        unsafe {
            tcsetattr(STDIN, TCSAFLUSH, &self.original);
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.restore();
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PgUp,
    PgDn,
    Enter,
    Esc,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Space,
    Char(char),
    CtrlC,
    Unknown,
}

fn decode(bytes: &[u8]) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            0x1b => {
                if i + 1 >= bytes.len() {
                    keys.push(Key::Esc);
                    i += 1;
                    continue;
                }
                if bytes[i + 1] == b'[' || bytes[i + 1] == b'O' {
                    let mut j = i + 2;
                    while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
                        j += 1;
                    }
                    let end = j.min(bytes.len() - 1);
                    let seq = &bytes[i + 1..=end];
                    keys.push(map_csi(seq));
                    i = end + 1;
                } else {
                    keys.push(Key::Esc);
                    i += 1;
                }
            }
            0x03 => {
                keys.push(Key::CtrlC);
                i += 1;
            }
            b'\r' | b'\n' => {
                keys.push(Key::Enter);
                i += 1;
            }
            b'\t' => {
                keys.push(Key::Tab);
                i += 1;
            }
            0x7f | 0x08 => {
                keys.push(Key::Backspace);
                i += 1;
            }
            b' ' => {
                keys.push(Key::Space);
                i += 1;
            }
            0x21..=0x7e => {
                keys.push(Key::Char(b as char));
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    keys
}

fn map_csi(seq: &[u8]) -> Key {
    // seq starts with '[' or 'O'.
    if seq.len() < 2 {
        return Key::Unknown;
    }
    match seq[seq.len() - 1] {
        b'A' => Key::Up,
        b'B' => Key::Down,
        b'C' => Key::Right,
        b'D' => Key::Left,
        b'H' => Key::Home,
        b'F' => Key::End,
        b'Z' => Key::BackTab,
        b'~' => {
            let body = &seq[1..seq.len() - 1];
            let n = std::str::from_utf8(body)
                .ok()
                .and_then(|s| s.split(';').next())
                .and_then(|s| s.parse::<u8>().ok())
                .unwrap_or(0);
            match n {
                1 | 7 => Key::Home,
                2 => Key::Unknown,
                3 => Key::Delete,
                4 | 8 => Key::End,
                5 => Key::PgUp,
                6 => Key::PgDn,
                _ => Key::Unknown,
            }
        }
        _ => Key::Unknown,
    }
}

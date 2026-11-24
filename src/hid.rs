//! Discovery of hidraw nodes and feature-report I/O via ioctl.
//! No hidapi, no libusb, no C dependency beyond the system libc.

use std::ffi::{c_int, c_ulong};
use std::fs::{self, File};
use std::io;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

pub const VENDORS: [u16; 2] = [0x5253, 0x3837];

const IOC_READ: u32 = 2;
const IOC_WRITE: u32 = 1;

const fn ioc(dir: u32, ty: u8, nr: u8, size: u32) -> c_ulong {
    ((dir as c_ulong) << 30) | ((size as c_ulong) << 16) | ((ty as c_ulong) << 8) | (nr as c_ulong)
}

fn hidioc_sfeature(len: u32) -> c_ulong {
    ioc(IOC_READ | IOC_WRITE, b'H', 0x06, len)
}

fn hidioc_gfeature(len: u32) -> c_ulong {
    ioc(IOC_READ | IOC_WRITE, b'H', 0x07, len)
}

extern "C" {
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}

pub struct HidRaw {
    file: File,
}

impl HidRaw {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = fs::OpenOptions::new().read(true).write(true).open(path)?;
        Ok(Self { file })
    }

    /// SET_REPORT(Feature). buf[0] is the report ID.
    pub fn set_feature(&self, buf: &[u8]) -> io::Result<()> {
        let rc = unsafe {
            ioctl(
                self.file.as_raw_fd(),
                hidioc_sfeature(buf.len() as u32),
                buf.as_ptr(),
            )
        };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// GET_REPORT(Feature). buf[0] carries the requested report ID in.
    pub fn get_feature(&self, buf: &mut [u8]) -> io::Result<usize> {
        let rc = unsafe {
            ioctl(
                self.file.as_raw_fd(),
                hidioc_gfeature(buf.len() as u32),
                buf.as_mut_ptr(),
            )
        };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(rc as usize)
    }
}

#[derive(Clone)]
pub struct Node {
    pub path: PathBuf,
    pub vid: u16,
    pub pid: u16,
    pub name: String,
    pub descriptor: Vec<u8>,
}

impl Node {
    pub fn is_config(&self) -> bool {
        is_config_descriptor(&self.descriptor)
    }

    pub fn is_mchose(&self) -> bool {
        VENDORS.contains(&self.vid) || self.name.to_ascii_uppercase().contains("MCHOSE")
    }
}

/// All hidraw nodes, cheapest sysfs fields first.
pub fn nodes() -> io::Result<Vec<Node>> {
    let dir = match fs::read_dir("/sys/class/hidraw") {
        Ok(d) => d,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut entries: Vec<_> = dir.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());

    let mut out = Vec::new();
    for entry in entries {
        let sys = entry.path();
        let uevent = fs::read_to_string(sys.join("device/uevent")).unwrap_or_default();
        let mut vid = 0u16;
        let mut pid = 0u16;
        let mut name = String::new();
        for line in uevent.lines() {
            if let Some(id) = line.strip_prefix("HID_ID=") {
                let mut parts = id.split(':');
                parts.next();
                vid = parts
                    .next()
                    .and_then(|v| u32::from_str_radix(v, 16).ok())
                    .unwrap_or(0) as u16;
                pid = parts
                    .next()
                    .and_then(|v| u32::from_str_radix(v, 16).ok())
                    .unwrap_or(0) as u16;
            } else if let Some(n) = line.strip_prefix("HID_NAME=") {
                name = n.trim().to_string();
            }
        }
        let descriptor = fs::read(sys.join("device/report_descriptor")).unwrap_or_default();
        out.push(Node {
            path: PathBuf::from("/dev").join(entry.file_name()),
            vid,
            pid,
            name,
            descriptor,
        });
    }
    Ok(out)
}

/// True when the report descriptor declares the vendor collection this
/// protocol lives on and both feature reports. G3-family mice have no
/// feature reports and are therefore rejected here.
pub fn is_config_descriptor(d: &[u8]) -> bool {
    let vendor_page = d
        .windows(3)
        .any(|w| w == [0x06, 0x01, 0xFF] || w == [0x06, 0x0B, 0xFF]);
    let report_11 = d.windows(2).any(|w| w == [0x85, 0x11]);
    let report_12 = d.windows(2).any(|w| w == [0x85, 0x12]);
    vendor_page && report_11 && report_12
}

/// The node name without the kernel's HID driver prefix, for display.
pub fn clean_name(name: &str) -> String {
    name.trim()
        .strip_prefix("RealTek ")
        .unwrap_or(name.trim())
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_descriptor_needs_vendor_page_and_both_reports() {
        assert!(is_config_descriptor(&[
            0x06, 0x0b, 0xff, 0x85, 0x11, 0x85, 0x12
        ]));
        assert!(!is_config_descriptor(&[0x06, 0x0b, 0xff, 0x85, 0x11]));
        assert!(!is_config_descriptor(&[
            0x06, 0x00, 0xff, 0x85, 0x11, 0x85, 0x12
        ]));
    }
}

//! Persistent state: the original config backup and the write-only LOD value.

use crate::proto::Config;
use std::fs;
use std::io;
use std::path::PathBuf;

fn base() -> PathBuf {
    let root = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir);
    root.join("mchose-tui")
}

pub fn original_path() -> PathBuf {
    base().join("config.original.bin")
}

pub fn backup_path() -> PathBuf {
    base().join("config.backup.bin")
}

fn lod_path(pid: u16) -> PathBuf {
    base().join(format!("lod-{pid:04x}.txt"))
}

/// Save the config as it was before this tool ever wrote to it. Never
/// overwrites an existing copy.
pub fn backup_once(c: &Config) -> io::Result<()> {
    let path = original_path();
    if path.exists() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, c.body)
}

pub fn backup_now(c: &Config) -> io::Result<PathBuf> {
    let path = backup_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&path, c.body)?;
    Ok(path)
}

pub fn load_original() -> io::Result<Config> {
    let bytes = fs::read(original_path())?;
    if bytes.len() != 63 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} is {} bytes, expected 63",
                original_path().display(),
                bytes.len()
            ),
        ));
    }
    let mut body = [0u8; 63];
    body.copy_from_slice(&bytes);
    Ok(Config { body })
}

pub fn load_lod(pid: u16, default: u8) -> u8 {
    fs::read_to_string(lod_path(pid))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(default)
}

pub fn save_lod(pid: u16, index: u8) {
    let path = lod_path(pid);
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(path, index.to_string());
}

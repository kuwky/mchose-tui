mod app;
mod buttons;
mod hid;
mod macros;
mod mouse;
mod proto;
mod state;
mod term;

use app::Options;
use std::io::ErrorKind;
use std::process::ExitCode;

const USAGE: &str = "\
mchose-tui - configure MCHOSE gaming mice from a terminal

Usage:
  mchose-tui [--demo] [--device /dev/hidrawN] [--no-color] [--accent #RRGGBB]
  mchose-tui --export FILE [--device /dev/hidrawN]
  mchose-tui --import FILE [--device /dev/hidrawN]
  mchose-tui --dump [--device /dev/hidrawN]
  mchose-tui --list
  mchose-tui --help

Options:
  --demo              run with a simulated L7 Ultra (no hardware needed)
  --device <path>     use this hidraw node instead of autodetection
  --no-color          disable ANSI colours
  --accent #RRGGBB    TUI accent colour (default: follow the DPI flash colour)
  --export FILE       write the 63-byte config block to FILE and exit
  --import FILE       restore a 63-byte config block from FILE and exit
  --dump              print every setting as plain text and exit
  --list              list MCHOSE hidraw nodes and exit
  -h, --help          show this help
  -V, --version       show the version

Keys:
  Up/Down or k/j      move          Left/Right or h/l   adjust
  Enter or Space      apply/edit    r                   refresh
  b                   backup        d                   config hex + diff
  f                   frame log     q                   quit

Press Enter on a button row to remap it. In the picker, type to filter,
Up/Down to choose, Enter to apply, Esc to cancel. Choose 'Create / edit
macro…' to open the macro editor.

Macro syntax: ctrl+c, shift+f10, alt+tab, enter, f5, delay:50,
mouse:left, mouse:right, wheel:up, and 'text' in quotes to type a string.
A short macro holds 13 events (52 bytes); one key is two events, a chord four.

The bottom LED briefly flashes the DPI-stage palette after a stage change,
then returns to a steady status/charge colour. The firmware ignores lighting
commands on this model, so neither colour can be set over HID. The TUI accent
can follow the flash palette or be set with --accent or the 'UI accent' row.

The mouse's configuration interface is usually root-only. Install the
included udev rule once (./install.sh does it), then replug the mouse.

Unofficial. Not affiliated with MCHOSE. No firmware is ever written.
";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mchose-tui: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut opts = Options {
        demo: false,
        device: None,
        color: true,
        accent: None,
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut export = None;
    let mut import = None;
    let mut dump = false;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--demo" => opts.demo = true,
            "--no-color" => opts.color = false,
            "--device" => {
                i += 1;
                let path = args
                    .get(i)
                    .ok_or_else(|| "--device needs a path".to_string())?;
                opts.device = Some(path.clone());
            }
            s if s.starts_with("--device=") => {
                opts.device = Some(s["--device=".len()..].to_string());
            }
            "--export" => {
                i += 1;
                export = Some(
                    args.get(i)
                        .ok_or_else(|| "--export needs a file".to_string())?
                        .clone(),
                );
            }
            s if s.starts_with("--export=") => {
                export = Some(s["--export=".len()..].to_string());
            }
            "--import" => {
                i += 1;
                import = Some(
                    args.get(i)
                        .ok_or_else(|| "--import needs a file".to_string())?
                        .clone(),
                );
            }
            s if s.starts_with("--import=") => {
                import = Some(s["--import=".len()..].to_string());
            }
            "--dump" => dump = true,
            "--accent" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "--accent needs #RRGGBB".to_string())?;
                opts.accent = Some(
                    app::parse_hex_color(value)
                        .ok_or_else(|| format!("--accent wants #RRGGBB, got {value:?}"))?,
                );
            }
            s if s.starts_with("--accent=") => {
                let value = &s["--accent=".len()..];
                opts.accent = Some(
                    app::parse_hex_color(value)
                        .ok_or_else(|| format!("--accent wants #RRGGBB, got {value:?}"))?,
                );
            }
            "--list" => {
                list()?;
                return Ok(());
            }
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("mchose-tui {}", app::VERSION);
                return Ok(());
            }
            other => return Err(format!("unknown argument {other:?}; try --help")),
        }
        i += 1;
    }
    if export.is_some() && import.is_some() {
        return Err("--export and --import cannot be used together".into());
    }
    if dump && (export.is_some() || import.is_some()) {
        return Err("--dump cannot be used with --export or --import".into());
    }
    if let Some(path) = export {
        return export_config(&opts, &path);
    }
    if let Some(path) = import {
        return import_config(&opts, &path);
    }
    if dump {
        return dump_config(&opts);
    }
    run_tui(opts)
}

fn dump_config(opts: &Options) -> Result<(), String> {
    println!("{}", app::dump(opts)?);
    Ok(())
}

fn export_config(opts: &Options, path: &str) -> Result<(), String> {
    let mut mouse = app::open_mouse(opts)?;
    let config = mouse
        .read_config()
        .map_err(|e| format!("cannot read the config: {e}"))?;
    std::fs::write(path, config.body).map_err(|e| format!("cannot write {path}: {e}"))?;
    println!("Exported the 63-byte config block to {path}.");
    Ok(())
}

fn import_config(opts: &Options, path: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    if bytes.len() != 63 {
        return Err(format!(
            "{path} is {} bytes; a config block must be exactly 63 bytes",
            bytes.len()
        ));
    }
    if bytes.iter().all(|b| *b == 0) {
        return Err(format!("{path} is all zeroes; refusing to import it"));
    }
    let mut body = [0u8; 63];
    body.copy_from_slice(&bytes);
    let wanted = proto::Config { body };

    let mut mouse = app::open_mouse(opts)?;
    let current = mouse
        .read_config()
        .map_err(|e| format!("cannot read the current config: {e}"))?;
    if !opts.demo {
        state::backup_once(&current)
            .map_err(|e| format!("cannot save the original config: {e}"))?;
    }
    let diff = current.diff(&wanted);
    mouse
        .write_config(&wanted)
        .map_err(|e| format!("cannot write the config: {e}"))?;
    mouse
        .confirm_config(&wanted)
        .map_err(|e| format!("the mouse did not take the config: {e}"))?;
    if diff.is_empty() {
        println!("Imported {path}; the config was already identical.");
    } else {
        println!("Imported {path}. Changed: {diff}");
    }
    Ok(())
}

fn list() -> Result<(), String> {
    let nodes = hid::nodes().map_err(|e| format!("cannot scan hidraw: {e}"))?;
    let mut found = false;
    for node in nodes {
        if !node.is_mchose() && !node.is_config() {
            continue;
        }
        found = true;
        let kind = if node.is_config() {
            "config interface"
        } else {
            "other interface"
        };
        let access = match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&node.path)
        {
            Ok(_) => "readable",
            Err(e) if e.kind() == ErrorKind::PermissionDenied => "permission denied",
            Err(_) => "unreadable",
        };
        println!(
            "{}  {:04x}:{:04x}  {}  [{}; {}]",
            node.path.display(),
            node.vid,
            node.pid,
            node.name,
            kind,
            access
        );
    }
    if !found {
        println!("No MCHOSE hidraw nodes found (vendor 5253 or 3837).");
    }
    Ok(())
}

fn run_tui(opts: Options) -> Result<(), String> {
    let term = term::Terminal::enter().map_err(|e| e.to_string())?;
    let (mut app, mut error) = match app::open(&opts) {
        Ok(app) => (Some(app), None),
        Err(e) => (None, Some(e)),
    };
    let mut dirty = true;
    let mut last_size = (0usize, 0usize);

    loop {
        let (width, height) = term.size();
        if (width, height) != last_size {
            last_size = (width, height);
            dirty = true;
        }
        let frame = match &app {
            Some(app) => app.render(width, height),
            None => app::render_error(
                error.as_deref().unwrap_or("Unknown startup error"),
                width,
                height,
                opts.color,
            ),
        };
        if dirty {
            term.draw(&frame).map_err(|e| e.to_string())?;
        }

        let keys = term.keys().map_err(|e| e.to_string())?;
        dirty = !keys.is_empty();
        let mut quit = false;
        let mut retry = false;

        if let Some(app) = app.as_mut() {
            for key in keys {
                if app.handle_key(key) {
                    quit = true;
                    break;
                }
            }
            if !quit && app.tick() {
                dirty = true;
            }
        } else {
            for key in keys {
                match key {
                    term::Key::Char('q') | term::Key::CtrlC => quit = true,
                    term::Key::Char('r') => retry = true,
                    _ => {}
                }
            }
        }

        if retry {
            match app::open(&opts) {
                Ok(opened) => {
                    app = Some(opened);
                    error = None;
                }
                Err(e) => error = Some(e),
            }
            dirty = true;
        }
        if quit {
            break;
        }
    }
    Ok(())
}

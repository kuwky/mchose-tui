mod app;
mod buttons;
mod hid;
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
  mchose-tui --list
  mchose-tui --help

Options:
  --demo              run with a simulated L7 Ultra (no hardware needed)
  --device <path>     use this hidraw node instead of autodetection
  --no-color          disable ANSI colours
  --accent #RRGGBB    TUI accent colour (default: follow the DPI LED colour)
  --list              list MCHOSE hidraw nodes and exit
  -h, --help          show this help
  -V, --version       show the version

Keys:
  Up/Down or k/j      move          Left/Right or h/l   adjust
  Enter or Space      apply/edit    r                   refresh
  b                   backup        d                   config hex + diff
  f                   frame log     q                   quit

Press Enter on a button row to remap it. In the picker, type to filter,
Up/Down to choose, Enter to apply, Esc to cancel.

The mouse's bottom status LED colour is chosen by the firmware from the
active DPI stage and cannot be changed over HID. The TUI accent can follow
that colour or be set with --accent or the 'UI accent' row.

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
    run_tui(opts)
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

//! Application state, key handling and the renderer.

use crate::buttons;
use crate::macros;
use crate::mouse::{model_info, DemoMouse, ModelInfo, Mouse, RealMouse};
use crate::proto::{self, Config, Identity, MacroEvent, BUTTON_COUNT, DPI_STAGES};
use crate::state;
use crate::term::Key;
use std::io;
use std::thread::sleep;
use std::time::{Duration, Instant};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy, PartialEq, Eq)]
enum MsgKind {
    Info,
    Ok,
    Err,
}

struct Message {
    text: String,
    kind: MsgKind,
}

impl Default for Message {
    fn default() -> Self {
        Self {
            text: "Ready. Changes are written immediately and verified.".into(),
            kind: MsgKind::Info,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    Stage,
    Dpi(u8),
    Rate,
    Rotation,
    Lod,
    Button(u8),
    Debounce,
    Sleep,
    Motion,
    Ripple,
    Snap,
    LedFollow,
    Accent,
    Profile,
    Game,
    Backup,
    Restore,
    Config,
    Log,
    Refresh,
}

struct Item {
    group: &'static str,
    row: Option<Row>,
    label: String,
    value: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditTarget {
    Dpi(u8),
    Rotation,
    Accent,
}

struct Edit {
    target: EditTarget,
    buffer: String,
}

struct Remap {
    slot: u8,
    query: String,
    selected: usize,
}

struct MacroEdit {
    slot: u8,
    buffer: String,
    error: Option<String>,
}

pub struct Options {
    pub demo: bool,
    pub device: Option<String>,
    pub color: bool,
    pub accent: Option<[u8; 3]>,
}

const DEFAULT_ACCENT: [u8; 3] = [0, 200, 220];

pub struct App {
    mouse: Box<dyn Mouse>,
    model: ModelInfo,
    identity: Identity,
    config: Config,
    version: String,
    game_mode: u8,
    lod_index: u8,
    selected: usize,
    edit: Option<Edit>,
    remap: Option<Remap>,
    macro_edit: Option<MacroEdit>,
    macro_names: [Option<String>; BUTTON_COUNT],
    macro_expr: [Option<String>; BUTTON_COUNT],
    message: Message,
    confirm_restore: bool,
    show_log: bool,
    show_config: bool,
    accent_override: Option<[u8; 3]>,
    accent_follows: bool,
    color: bool,
    demo: bool,
    last_poll: Instant,
    battery_stale: bool,
}

pub fn open_mouse(opts: &Options) -> Result<Box<dyn Mouse>, String> {
    if opts.demo {
        Ok(Box::new(DemoMouse::new()))
    } else {
        Ok(Box::new(RealMouse::discover(opts.device.as_deref())?))
    }
}

pub fn open(opts: &Options) -> Result<App, String> {
    let mouse = open_mouse(opts)?;
    App::new(mouse, opts.demo, opts.color, opts.accent).map_err(|e| {
        format!(
            "The interface was found but the mouse did not answer.\n\n{e}\n\n\
             Move the mouse to wake it, then press r to retry."
        )
    })
}

/// Read every setting and render it as plain, aligned text. The format is
/// deliberately simple so it can be diffed or parsed by a script.
pub fn dump(opts: &Options) -> Result<String, String> {
    let mut mouse = open_mouse(opts)?;
    let identity = mouse
        .identity()
        .map_err(|e| format!("cannot read the mouse identity: {e}"))?;
    let model = model_info(identity.pid, mouse.name());
    let version = mouse.version().unwrap_or_default();
    let config = mouse
        .read_config()
        .map_err(|e| format!("cannot read the config: {e}"))?;

    let stages = config.enabled_stages().clamp(1, DPI_STAGES as u8);
    let active = if identity.connect_mode == 0 {
        config.wired_dpi_stage()
    } else {
        config.wireless_dpi_stage()
    }
    .min(stages - 1);
    let rate_index = if identity.connect_mode == 0 {
        config.wired_rate_index()
    } else {
        config.wireless_rate_index()
    };
    let lod_index = if opts.demo {
        model.default_lod
    } else if model.lod_in_sensor {
        config
            .lod_index()
            .min(model.lod.len().saturating_sub(1) as u8)
    } else {
        state::load_lod(identity.pid, model.default_lod)
            .min(model.lod.len().saturating_sub(1) as u8)
    };

    let mut out: Vec<String> = Vec::new();
    out.push(mouse.name().to_string());
    field(
        &mut out,
        "Interface",
        format!(
            "{}  ({:04x}:{:04x})",
            mouse.path(),
            identity.vid,
            identity.pid
        ),
    );
    field(
        &mut out,
        "Link",
        format!(
            "{}  ·  {}",
            link_name(identity.connect_mode),
            if identity.connected {
                "online"
            } else {
                "offline"
            }
        ),
    );
    field(
        &mut out,
        "Firmware",
        if version.is_empty() {
            format!("0x{:08x}", identity.firmware)
        } else {
            version
        },
    );
    field(
        &mut out,
        "Battery",
        format!(
            "{}%{}",
            identity.battery,
            if identity.charging != 0 {
                "  charging"
            } else {
                ""
            }
        ),
    );

    section(&mut out, "PERFORMANCE");
    field(
        &mut out,
        "Active DPI stage",
        format!(
            "{} / {}  ({} DPI)",
            active + 1,
            stages,
            config.dpi(active as usize)
        ),
    );
    for stage in 0..stages {
        let x = config.dpi(stage as usize);
        let y = config.dpi_y(stage as usize);
        let value = if y != 0 && y != x {
            format!("{x} x {y} DPI")
        } else {
            format!("{x} DPI")
        };
        field(&mut out, &format!("DPI stage {}", stage + 1), value);
    }
    field(
        &mut out,
        "Polling rate",
        match model.rates.get(rate_index as usize) {
            Some(hz) => format!("{hz} Hz"),
            None => format!("index {rate_index}"),
        },
    );
    field(&mut out, "Debounce", format!("{} ms", config.debounce_ms()));
    field(
        &mut out,
        "Sleep",
        match config.sleep_minutes() {
            0 => "off".to_string(),
            n => format!("{n} min"),
        },
    );

    section(&mut out, "SENSOR");
    let lod = model.lod.get(lod_index as usize).copied().unwrap_or(0.0);
    field(
        &mut out,
        "Lift-off distance",
        format!(
            "{lod:.1} mm{}",
            if model.lod_in_sensor {
                ""
            } else {
                "  (write-only)"
            }
        ),
    );
    let rot = config.rotate_degrees();
    field(
        &mut out,
        "Sensor rotation",
        if rot == 0 {
            format!("0°  (raw {})", config.rotate())
        } else {
            format!("{rot:+}°  (raw {})", config.rotate())
        },
    );
    let (ripple, snap, sync) = config.perf_flags();
    field(&mut out, "Motion sync", on_off(sync));
    field(&mut out, "Ripple control", on_off(ripple));
    field(&mut out, "Angle snap", on_off(snap));

    section(&mut out, "BUTTONS");
    for info in buttons::BUTTONS {
        let (kind, value) = config.button(info.slot as usize);
        let text = if kind == buttons::KIND_MACRO {
            match mouse.macro_name(info.wire) {
                Ok(name) if !name.trim().is_empty() => format!("Macro · {name}"),
                _ => "Macro".to_string(),
            }
        } else {
            buttons::describe(kind, value)
        };
        field(&mut out, &format!("{} button", info.name), text);
    }

    section(&mut out, "PROFILE");
    field(&mut out, "Profile", format!("{} / 3", config.profile() + 1));
    field(&mut out, "Game mode", config.game_mode().to_string());

    section(&mut out, "RAW CONFIG");
    out.push(format!("  {}", proto::hex(&config.body)));

    Ok(out.join("\n"))
}

fn section(out: &mut Vec<String>, title: &str) {
    if !out.is_empty() {
        out.push(String::new());
    }
    out.push(title.to_string());
}

fn field(out: &mut Vec<String>, label: &str, value: impl Into<String>) {
    out.push(format!("  {label:<18}  {}", value.into()));
}

impl App {
    fn new(
        mut mouse: Box<dyn Mouse>,
        demo: bool,
        color: bool,
        accent: Option<[u8; 3]>,
    ) -> io::Result<Self> {
        let identity = mouse.identity()?;
        let model = model_info(identity.pid, mouse.name());
        let version = mouse.version().unwrap_or_default();
        let config = mouse.read_config()?;
        let game_mode = config.game_mode();
        let mut macro_names: [Option<String>; BUTTON_COUNT] = std::array::from_fn(|_| None);
        for (slot, name) in macro_names.iter_mut().enumerate() {
            if config.button(slot).0 == buttons::KIND_MACRO {
                if let Ok(read) = mouse.macro_name(slot as u8) {
                    if !read.trim().is_empty() {
                        *name = Some(read);
                    }
                }
            }
        }
        let pid = identity.pid;
        let macro_expr: [Option<String>; BUTTON_COUNT] = std::array::from_fn(|slot| {
            if demo {
                None
            } else {
                state::load_macro_expr(pid, slot as u8)
            }
        });
        let lod_index = if demo {
            model.default_lod
        } else if model.lod_in_sensor {
            config
                .lod_index()
                .min(model.lod.len().saturating_sub(1) as u8)
        } else {
            state::load_lod(identity.pid, model.default_lod)
                .min(model.lod.len().saturating_sub(1) as u8)
        };
        Ok(Self {
            mouse,
            model,
            identity,
            config,
            version,
            game_mode,
            lod_index,
            selected: 0,
            edit: None,
            remap: None,
            macro_edit: None,
            macro_names,
            macro_expr,
            message: Message::default(),
            confirm_restore: false,
            show_log: false,
            show_config: false,
            accent_override: accent,
            accent_follows: accent.is_none(),
            color,
            demo,
            last_poll: Instant::now(),
            battery_stale: false,
        })
    }

    pub fn tick(&mut self) -> bool {
        if self.last_poll.elapsed() < Duration::from_secs(15) {
            return false;
        }
        self.last_poll = Instant::now();
        match self.mouse.identity() {
            Ok(id) => {
                self.identity = id;
                self.battery_stale = false;
            }
            Err(_) => self.battery_stale = true,
        }
        true
    }

    /// Returns true when the app should quit.
    pub fn handle_key(&mut self, key: Key) -> bool {
        if self.macro_edit.is_some() {
            if matches!(key, Key::CtrlC) {
                return true;
            }
            self.handle_macro_edit(key);
            return false;
        }
        if self.remap.is_some() {
            if matches!(key, Key::CtrlC) {
                return true;
            }
            self.handle_remap(key);
            return false;
        }
        if self.edit.is_some() {
            self.handle_edit(key);
            return false;
        }
        if self.show_config {
            match key {
                Key::Char('d') | Key::Esc => self.show_config = false,
                Key::Char('r') => self.refresh(),
                Key::Char('q') | Key::CtrlC => return true,
                _ => {}
            }
            return false;
        }
        if self.confirm_restore && key != Key::Enter && key != Key::Esc {
            self.confirm_restore = false;
        }
        match key {
            Key::Up | Key::Char('k') => self.move_sel(-1),
            Key::Down | Key::Char('j') => self.move_sel(1),
            Key::Left | Key::Char('h') => self.adjust(-1),
            Key::Right | Key::Char('l') => self.adjust(1),
            Key::PgUp => self.adjust(5),
            Key::PgDn => self.adjust(-5),
            Key::Home => self.selected = 0,
            Key::End => self.selected = self.action_count().saturating_sub(1),
            Key::Enter | Key::Space => self.activate(),
            Key::Char('r') => self.refresh(),
            Key::Char('b') => self.backup(),
            Key::Char('f') => self.show_log = !self.show_log,
            Key::Char('d') => self.show_config = true,
            Key::Char('q') | Key::CtrlC => return true,
            Key::Esc if self.confirm_restore => {
                self.confirm_restore = false;
                self.info("Restore cancelled.");
            }
            Key::Esc => {}
            _ => {}
        }
        false
    }

    fn handle_edit(&mut self, key: Key) {
        let mut edit = match self.edit.take() {
            Some(e) => e,
            None => return,
        };
        match key {
            Key::Enter => match edit.target {
                EditTarget::Dpi(stage) => match edit.buffer.parse::<u16>() {
                    Ok(value) => self.apply_dpi(stage, value),
                    Err(_) => self.err("DPI must be a number."),
                },
                EditTarget::Rotation => match edit.buffer.parse::<i16>() {
                    Ok(degrees) => self.apply_rotation(degrees),
                    Err(_) => self.err("Rotation must be a whole number of degrees."),
                },
                EditTarget::Accent => match parse_hex_color(&edit.buffer) {
                    Some(rgb) => self.apply_accent(rgb),
                    None => self.err("Accent must be #RRGGBB."),
                },
            },
            Key::Esc => self.info("Edit cancelled."),
            Key::Backspace => {
                edit.buffer.pop();
                self.edit = Some(edit);
            }
            Key::Char(c) => {
                let allowed = match edit.target {
                    EditTarget::Dpi(_) => c.is_ascii_digit(),
                    EditTarget::Rotation => {
                        c.is_ascii_digit() || (c == '-' && edit.buffer.is_empty())
                    }
                    EditTarget::Accent => {
                        c.is_ascii_hexdigit() || (c == '#' && edit.buffer.is_empty())
                    }
                };
                let max = match edit.target {
                    EditTarget::Accent => 7,
                    _ => 5,
                };
                if allowed && edit.buffer.len() < max {
                    edit.buffer.push(c);
                }
                self.edit = Some(edit);
            }
            _ => self.edit = Some(edit),
        }
    }

    fn handle_remap(&mut self, key: Key) {
        let Some(mut remap) = self.remap.take() else {
            return;
        };
        let matches = buttons::filter(&remap.query);
        match key {
            Key::Esc => {
                self.info("Remap cancelled.");
                return;
            }
            Key::Enter => {
                if let Some(action) = matches.get(remap.selected).copied() {
                    if action.kind == buttons::KIND_MACRO {
                        self.open_macro_editor(remap.slot);
                    } else {
                        self.apply_button(remap.slot, action);
                    }
                } else {
                    self.remap = Some(remap);
                    self.info("No action selected.");
                }
                return;
            }
            Key::Up => remap.selected = remap.selected.saturating_sub(1),
            Key::Down => {
                if remap.selected + 1 < matches.len() {
                    remap.selected += 1;
                }
            }
            Key::PgUp => remap.selected = remap.selected.saturating_sub(10),
            Key::PgDn => {
                remap.selected = (remap.selected + 10).min(matches.len().saturating_sub(1))
            }
            Key::Home => remap.selected = 0,
            Key::End => remap.selected = matches.len().saturating_sub(1),
            Key::Backspace => {
                remap.query.pop();
                remap.selected = 0;
            }
            Key::Space => {
                remap.query.push(' ');
                remap.selected = 0;
            }
            Key::Char(c) if !c.is_control() => {
                remap.query.push(c);
                remap.selected = 0;
            }
            _ => {}
        }
        self.remap = Some(remap);
    }

    fn handle_macro_edit(&mut self, key: Key) {
        let Some(mut edit) = self.macro_edit.take() else {
            return;
        };
        match key {
            Key::Enter => match macros::parse(&edit.buffer) {
                Ok(events) => {
                    let expression = edit.buffer.trim().to_string();
                    self.apply_macro(edit.slot, &events, &expression);
                }
                Err(e) => {
                    edit.error = Some(e);
                    self.macro_edit = Some(edit);
                }
            },
            Key::Esc => self.info("Macro cancelled."),
            Key::Backspace => {
                edit.buffer.pop();
                edit.error = None;
                self.macro_edit = Some(edit);
            }
            Key::Space | Key::Tab => {
                edit.buffer.push(' ');
                edit.error = None;
                self.macro_edit = Some(edit);
            }
            Key::Char(c) if !c.is_control() => {
                if edit.buffer.len() < 400 {
                    edit.buffer.push(c);
                }
                edit.error = None;
                self.macro_edit = Some(edit);
            }
            _ => self.macro_edit = Some(edit),
        }
    }

    fn open_macro_editor(&mut self, slot: u8) {
        let Some(info) = buttons::button_info(slot) else {
            return;
        };
        self.remap = None;
        let buffer = self.macro_expr[slot as usize].clone().unwrap_or_default();
        self.macro_edit = Some(MacroEdit {
            slot,
            buffer,
            error: None,
        });
        let existing = self.macro_names[slot as usize].as_deref().unwrap_or("none");
        self.info(format!(
            "Macro for the {} button. On the mouse: {existing}. Enter saves, Esc cancels.",
            info.name
        ));
    }

    fn apply_macro(&mut self, slot: u8, events: &[MacroEvent], expression: &str) {
        self.macro_edit = None;
        let Some(info) = buttons::button_info(slot).copied() else {
            return;
        };
        let before = match self.mouse.read_config() {
            Ok(c) => c,
            Err(e) => {
                self.macro_failed(slot, expression, format!("Could not read the mouse: {e}"));
                return;
            }
        };
        let mut wanted = before.clone();
        wanted.set_button(slot as usize, buttons::KIND_MACRO, 0);
        if left_click_count(&wanted) == 0 {
            self.macro_failed(
                slot,
                expression,
                "Refusing to remove the last left click. Map another button to \
                 Left click first."
                    .into(),
            );
            return;
        }
        if let Err(e) = self.backup_once(&before) {
            self.macro_failed(
                slot,
                expression,
                format!("Could not save the original config: {e}"),
            );
            return;
        }

        let name = macros::name_for(expression);
        if let Err(e) = self.mouse.write_macro(info.wire, events, &name) {
            self.macro_failed(
                slot,
                expression,
                format!("{} macro: write failed ({e})", info.name),
            );
            return;
        }

        // Writing the macro usually binds the button on the firmware side. Only
        // send the explicit button command if the config still says otherwise;
        // this keeps the vendor's automatic path untouched.
        sleep(Duration::from_millis(120));
        let already_bound = self
            .mouse
            .read_config()
            .map(|config| config.button(slot as usize).0 == buttons::KIND_MACRO)
            .unwrap_or(false);
        if !already_bound {
            if let Err(e) = self.mouse.set_button(info.wire, buttons::KIND_MACRO, 0) {
                self.macro_failed(
                    slot,
                    expression,
                    format!("{} macro: binding failed ({e})", info.name),
                );
                return;
            }
        }

        for _ in 0..20 {
            sleep(Duration::from_millis(150));
            if let Ok(config) = self.mouse.read_config() {
                self.config = config;
                if self.config.button(slot as usize).0 == buttons::KIND_MACRO {
                    self.macro_expr[slot as usize] = Some(expression.to_string());
                    if !self.demo {
                        let _ = state::save_macro_expr(self.identity.pid, slot, expression);
                    }
                    self.macro_names[slot as usize] = Some(name.clone());
                    self.refresh_macro_names();
                    self.ok(format!(
                        "{} button: macro \"{}\" ({}).",
                        info.name,
                        name,
                        macros::summary(events)
                    ));
                    return;
                }
            }
        }

        self.reload();
        if self.config.button(slot as usize).0 == buttons::KIND_MACRO {
            self.macro_expr[slot as usize] = Some(expression.to_string());
            if !self.demo {
                let _ = state::save_macro_expr(self.identity.pid, slot, expression);
            }
            self.macro_names[slot as usize] = Some(name.clone());
            self.ok(format!(
                "{} button: macro \"{}\" ({}).",
                info.name,
                name,
                macros::summary(events)
            ));
        } else {
            let (kind, value) = self.config.button(slot as usize);
            self.macro_failed(
                slot,
                expression,
                format!(
                    "The mouse kept {} for the {} button.",
                    buttons::describe(kind, value),
                    info.name
                ),
            );
        }
    }

    fn macro_failed(&mut self, slot: u8, expression: &str, error: String) {
        self.err(error.clone());
        self.macro_edit = Some(MacroEdit {
            slot,
            buffer: expression.to_string(),
            error: Some(error),
        });
        self.reload();
    }

    fn refresh_macro_names(&mut self) {
        for (slot, name) in self.macro_names.iter_mut().enumerate() {
            if self.config.button(slot).0 == buttons::KIND_MACRO {
                if let Ok(read) = self.mouse.macro_name(slot as u8) {
                    if !read.trim().is_empty() {
                        *name = Some(read);
                    }
                }
            } else {
                *name = None;
            }
        }
    }

    fn open_remap(&mut self, slot: u8) {
        let Some(info) = buttons::button_info(slot) else {
            return;
        };
        self.remap = Some(Remap {
            slot,
            query: String::new(),
            selected: 0,
        });
        self.info(format!(
            "Remapping the {} button: type to filter, Enter to apply, Esc to cancel.",
            info.name
        ));
    }

    fn apply_button(&mut self, slot: u8, action: buttons::Action) {
        self.remap = None;
        let Some(info) = buttons::button_info(slot).copied() else {
            return;
        };
        let before = match self.mouse.read_config() {
            Ok(c) => c,
            Err(e) => {
                self.err(format!("Could not read the mouse: {e}"));
                return;
            }
        };
        let mut wanted = before.clone();
        wanted.set_button(slot as usize, action.kind, action.value);
        if left_click_count(&wanted) == 0 {
            self.err(
                "Refusing to remove the last left click. Map another button to \
                 Left click first.",
            );
            return;
        }
        if let Err(e) = self.backup_once(&before) {
            self.err(format!("Could not save the original config: {e}"));
            return;
        }
        if let Err(e) = self.mouse.set_button(info.wire, action.kind, action.value) {
            self.err(format!("{} button: failed ({e})", info.name));
            self.reload();
            return;
        }

        let mut last_error = None;
        let mut seen = None;
        for _ in 0..20 {
            sleep(Duration::from_millis(150));
            match self.mouse.read_config() {
                Ok(config) => {
                    seen = Some(config.button(slot as usize));
                    self.config = config;
                    if self.config.button(slot as usize) == (action.kind, action.value) {
                        self.ok(format!("{} button: {}.", info.name, action.name));
                        return;
                    }
                }
                Err(e) => last_error = Some(e),
            }
        }

        // The firmware can commit a remap slightly after the last poll. A
        // final read is cheap and turns a late write into a success instead
        // of a misleading error.
        self.reload();
        if self.config.button(slot as usize) == (action.kind, action.value) {
            self.ok(format!("{} button: {}.", info.name, action.name));
            return;
        }

        if let Some((kind, value)) = seen {
            self.err(format!(
                "The mouse kept {} for the {} button.",
                buttons::describe(kind, value),
                info.name
            ));
        } else if let Some(e) = last_error {
            self.err(format!("{} button: read-back failed ({e})", info.name));
        } else {
            self.err(format!(
                "The mouse did not take that {} button mapping.",
                info.name
            ));
        }
    }

    fn action_count(&self) -> usize {
        self.items().iter().filter(|i| i.row.is_some()).count()
    }

    fn move_sel(&mut self, delta: i32) {
        let max = self.action_count().saturating_sub(1) as i32;
        self.selected = (self.selected as i32 + delta).clamp(0, max) as usize;
    }

    fn selected_row(&self) -> Option<Row> {
        self.items()
            .into_iter()
            .filter_map(|i| i.row)
            .nth(self.selected)
    }

    fn activate(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        match row {
            Row::Dpi(stage) => self.start_dpi_edit(stage),
            Row::Rotation => self.start_rotation_edit(),
            Row::Accent => self.start_accent_edit(),
            Row::LedFollow => self.toggle_accent_follow(),
            Row::Button(slot) => self.open_remap(slot),
            Row::Backup => self.backup(),
            Row::Restore => self.restore_prompt(),
            Row::Config => self.show_config = true,
            Row::Log => self.show_log = !self.show_log,
            Row::Refresh => self.refresh(),
            Row::Motion | Row::Ripple | Row::Snap => self.toggle(row),
            _ => self.adjust(1),
        }
    }

    fn adjust(&mut self, delta: i32) {
        let Some(row) = self.selected_row() else {
            return;
        };
        if delta == 0 {
            return;
        }
        match row {
            Row::Stage => {
                let n = self.enabled_stages() as i32;
                let next = (self.active_stage() as i32 + delta).rem_euclid(n) as u8;
                self.apply_stage(next);
            }
            Row::Dpi(stage) => {
                let current = self.config.dpi(stage as usize) as i32;
                let max = self.model.dpi_max as i32;
                let mut value = (current + delta * 50).clamp(50, max);
                value = (value / 50) * 50;
                if value as u16 != current as u16 {
                    self.apply_dpi(stage, value as u16);
                }
            }
            Row::Rate => {
                let n = self.model.rates.len() as i32;
                if n == 0 {
                    return;
                }
                let next = (self.active_rate_index() as i32 + delta).rem_euclid(n) as usize;
                self.apply_rate(next);
            }
            Row::Rotation => {
                let current = self.config.rotate_degrees();
                let next = (current as i32 + delta).clamp(-30, 30) as i16;
                if next != current {
                    self.apply_rotation(next);
                }
            }
            Row::Lod => {
                let n = self.model.lod.len() as i32;
                if n == 0 {
                    return;
                }
                let next = (self.lod_index as i32 + delta).rem_euclid(n) as u8;
                self.apply_lod(next);
            }
            Row::Debounce => {
                let current = self.config.debounce_ms() as i32;
                let next = (current + delta).clamp(0, 30) as u8;
                if next as i32 != current {
                    self.apply_debounce(next);
                }
            }
            Row::Sleep => {
                let current = self.config.sleep_minutes() as i32;
                let next = (current + delta).clamp(0, 60) as u8;
                if next as i32 != current {
                    self.apply_sleep(next);
                }
            }
            Row::Profile => {
                let next = (self.config.profile() as i32 + delta).rem_euclid(3) as u8;
                self.apply_profile(next + 1);
            }
            Row::Game => {
                let next = ((self.game_mode as i32 - 1 + delta).rem_euclid(3) + 1) as u8;
                self.apply_game(next);
            }
            Row::LedFollow => self.toggle_accent_follow(),
            Row::Motion | Row::Ripple | Row::Snap => self.toggle(row),
            _ => {}
        }
    }

    fn start_dpi_edit(&mut self, stage: u8) {
        self.edit = Some(Edit {
            target: EditTarget::Dpi(stage),
            buffer: self.config.dpi(stage as usize).to_string(),
        });
        self.message = Message {
            text: format!(
                "Editing DPI stage {}: type a value, Enter to apply, Esc to cancel.",
                stage + 1
            ),
            kind: MsgKind::Info,
        };
    }

    fn start_rotation_edit(&mut self) {
        self.edit = Some(Edit {
            target: EditTarget::Rotation,
            buffer: self.config.rotate_degrees().to_string(),
        });
        self.info("Editing sensor rotation in degrees (-30 to 30): Enter apply, Esc cancel.");
    }

    fn start_accent_edit(&mut self) {
        self.edit = Some(Edit {
            target: EditTarget::Accent,
            buffer: hex_rgb(self.accent_color()),
        });
        self.info("Editing the TUI accent colour (not the mouse LED): #RRGGBB, Enter to apply.");
    }

    fn apply_accent(&mut self, rgb: [u8; 3]) {
        self.accent_override = Some(rgb);
        self.accent_follows = false;
        self.ok(format!(
            "TUI accent: {}  (the mouse LED is fixed by the firmware).",
            hex_rgb(rgb)
        ));
    }

    fn toggle_accent_follow(&mut self) {
        self.accent_follows = !self.accent_follows;
        if self.accent_follows {
            self.ok(format!(
                "TUI accent follows the DPI flash colour: {} now.",
                led_color_name(self.active_stage())
            ));
        } else {
            self.ok(format!(
                "TUI accent fixed at {}.",
                hex_rgb(self.accent_color())
            ));
        }
    }

    fn accent_color(&self) -> [u8; 3] {
        if self.accent_follows {
            stage_color(self.active_stage())
        } else {
            self.accent_override.unwrap_or(DEFAULT_ACCENT)
        }
    }

    fn accent_code(&self) -> String {
        rgb_code(self.accent_color(), true)
    }

    fn edit_config<F: FnOnce(&mut Config)>(&mut self, what: &str, change: F) {
        let mut config = match self.mouse.read_config() {
            Ok(c) => c,
            Err(e) => {
                self.err(format!("Could not read the mouse: {e}"));
                return;
            }
        };
        if let Err(e) = self.backup_once(&config) {
            self.err(format!("Could not save the original config: {e}"));
            return;
        }
        change(&mut config);
        let result = match self.mouse.write_config(&config) {
            Ok(()) => self.mouse.confirm_config(&config),
            Err(e) => Err(e),
        };
        match result {
            Ok(()) => {
                self.ok(format!("{what}: applied."));
                self.reload();
            }
            Err(e) => {
                self.err(format!("{what}: failed ({e})"));
                self.reload();
            }
        }
    }

    fn backup_once(&self, config: &Config) -> io::Result<()> {
        if self.demo {
            Ok(())
        } else {
            state::backup_once(config)
        }
    }

    fn reload(&mut self) {
        if let Ok(config) = self.mouse.read_config() {
            self.game_mode = config.game_mode();
            self.config = config;
            self.sync_lod_from_config();
            self.refresh_macro_names();
        }
    }

    /// On models that mirror LOD in the sensor byte, the config block is the
    /// source of truth; on the others the last written index is all we have.
    fn sync_lod_from_config(&mut self) {
        if self.model.lod_in_sensor {
            let max = self.model.lod.len().saturating_sub(1) as u8;
            self.lod_index = self.config.lod_index().min(max);
        }
    }

    fn apply_stage(&mut self, stage: u8) {
        if stage >= self.enabled_stages() {
            return;
        }
        self.edit_config("Active DPI stage", |c| c.set_dpi_stage(stage));
    }

    fn apply_dpi(&mut self, stage: u8, value: u16) {
        let max = self.model.dpi_max;
        if value < 50 || value > max || !value.is_multiple_of(50) {
            self.err(format!("DPI must be 50-{max}, in steps of 50."));
            return;
        }
        self.edit_config(&format!("DPI stage {}", stage + 1), |c| {
            c.set_dpi(stage as usize, value)
        });
    }

    fn apply_rate(&mut self, index: usize) {
        if self.model.rates.is_empty() {
            return;
        }
        let index = index.min(self.model.rates.len() - 1);
        let before = match self.mouse.read_config() {
            Ok(c) => c,
            Err(e) => {
                self.err(format!("Could not read the mouse: {e}"));
                return;
            }
        };
        if let Err(e) = self.backup_once(&before) {
            self.err(format!("Could not save the original config: {e}"));
            return;
        }
        if let Err(e) = self.mouse.set_rate(index as u8) {
            self.err(format!("Polling rate: failed ({e})"));
            self.reload();
            return;
        }
        sleep(Duration::from_millis(80));
        match self.mouse.read_config() {
            Ok(config) => {
                let landed = config.wired_rate_index() as usize == index
                    || config.wireless_rate_index() as usize == index;
                if landed {
                    self.config = config;
                    self.ok(format!("Polling rate: {} Hz.", self.model.rates[index]));
                } else {
                    self.err("The mouse did not take that polling rate.");
                    self.reload();
                }
            }
            Err(e) => {
                self.err(format!("Polling rate: read-back failed ({e})"));
                self.reload();
            }
        }
    }

    fn apply_lod(&mut self, index: u8) {
        if index as usize >= self.model.lod.len() {
            return;
        }
        let (ripple, snap, sync) = self.config.perf_flags();
        self.apply_flags(ripple, snap, sync, index);
    }

    fn apply_rotation(&mut self, degrees: i16) {
        let degrees = degrees.clamp(-30, 30);
        let before = match self.mouse.read_config() {
            Ok(c) => c,
            Err(e) => {
                self.err(format!("Could not read the mouse: {e}"));
                return;
            }
        };
        if let Err(e) = self.backup_once(&before) {
            self.err(format!("Could not save the original config: {e}"));
            return;
        }
        // Encode through Config so the signed byte math lives in one place.
        let mut probe = before.clone();
        probe.set_rotate_degrees(degrees);
        let raw = probe.rotate();
        let degrees = probe.rotate_degrees();
        let (ripple, snap, sync) = before.perf_flags();
        if let Err(e) = self
            .mouse
            .set_flags(self.lod_index, ripple, snap, sync, raw)
        {
            self.err(format!("Sensor rotation: failed ({e})"));
            self.reload();
            return;
        }
        sleep(Duration::from_millis(120));
        match self.mouse.read_config() {
            Ok(config) => {
                self.config = config;
                self.sync_lod_from_config();
                if self.config.rotate() != raw {
                    self.err(format!(
                        "The mouse kept {}°, not {degrees}° (raw {} vs {raw}).",
                        self.config.rotate_degrees(),
                        self.config.rotate()
                    ));
                    return;
                }
                self.ok(format!(
                    "Sensor rotation: {}° (raw {}).",
                    self.config.rotate_degrees(),
                    raw
                ));
            }
            Err(e) => {
                self.err(format!("Sensor rotation: read-back failed ({e})"));
                self.reload();
            }
        }
    }

    fn apply_flags(&mut self, ripple: bool, snap: bool, sync: bool, lod: u8) {
        let before = match self.mouse.read_config() {
            Ok(c) => c,
            Err(e) => {
                self.err(format!("Could not read the mouse: {e}"));
                return;
            }
        };
        if let Err(e) = self.backup_once(&before) {
            self.err(format!("Could not save the original config: {e}"));
            return;
        }
        let rotate = before.rotate();
        if let Err(e) = self.mouse.set_flags(lod, ripple, snap, sync, rotate) {
            self.err(format!("Sensor settings: failed ({e})"));
            self.reload();
            return;
        }
        match self.mouse.read_config() {
            Ok(config) => {
                self.config = config;
                if self.config.perf_flags() != (ripple, snap, sync) {
                    self.err("The sensor flags did not stick.");
                    return;
                }
                let max = self.model.lod.len().saturating_sub(1) as u8;
                let actual = if self.model.lod_in_sensor {
                    self.config.lod_index().min(max)
                } else {
                    lod
                };
                if self.model.lod_in_sensor && actual != lod {
                    self.lod_index = actual;
                    if !self.demo {
                        state::save_lod(self.identity.pid, actual);
                    }
                    self.err(format!(
                        "Lift-off: the mouse kept {:.1} mm, not {:.1} mm.",
                        self.model.lod.get(actual as usize).copied().unwrap_or(0.0),
                        self.model.lod.get(lod as usize).copied().unwrap_or(0.0)
                    ));
                    return;
                }
                self.lod_index = actual;
                if !self.demo {
                    state::save_lod(self.identity.pid, actual);
                }
                self.ok("Sensor settings applied.");
            }
            Err(e) => {
                self.err(format!("Sensor settings: read-back failed ({e})"));
                self.reload();
            }
        }
    }

    fn toggle(&mut self, row: Row) {
        let (ripple, snap, sync) = self.config.perf_flags();
        let (ripple, snap, sync) = match row {
            Row::Ripple => (!ripple, snap, sync),
            Row::Snap => (ripple, !snap, sync),
            Row::Motion => (ripple, snap, !sync),
            _ => return,
        };
        self.apply_flags(ripple, snap, sync, self.lod_index);
    }

    fn apply_debounce(&mut self, ms: u8) {
        self.edit_config("Debounce", |c| c.set_debounce_ms(ms));
    }

    fn apply_sleep(&mut self, minutes: u8) {
        self.edit_config("Sleep", |c| c.set_sleep_minutes(minutes));
    }

    fn apply_profile(&mut self, profile: u8) {
        let before = match self.mouse.read_config() {
            Ok(c) => c,
            Err(e) => {
                self.err(format!("Could not read the mouse: {e}"));
                return;
            }
        };
        if let Err(e) = self.backup_once(&before) {
            self.err(format!("Could not save the original config: {e}"));
            return;
        }
        if let Err(e) = self.mouse.set_profile(profile - 1) {
            self.err(format!("Profile: failed ({e})"));
            self.reload();
            return;
        }
        let mut profile_seen = false;
        for _ in 0..5 {
            sleep(Duration::from_millis(80));
            if let Ok(config) = self.mouse.read_config() {
                if config.profile() + 1 == profile {
                    self.config = config;
                    profile_seen = true;
                    break;
                }
            }
        }
        if profile_seen {
            self.ok(format!("Profile {profile} active."));
        } else {
            self.err("The mouse did not switch profile.");
            self.reload();
        }
    }

    fn apply_game(&mut self, mode: u8) {
        if let Err(e) = self.mouse.set_game_mode(mode) {
            self.err(format!("Game mode: failed ({e})"));
            return;
        }
        for attempt in 0..5 {
            sleep(Duration::from_millis(60 * (attempt + 1)));
            match self.mouse.read_config() {
                Ok(config) if config.game_mode() == mode => {
                    self.config = config;
                    self.game_mode = mode;
                    self.sync_lod_from_config();
                    self.ok(format!("Game mode {mode}."));
                    return;
                }
                Ok(_) => {}
                Err(e) => {
                    if attempt == 4 {
                        self.err(format!("Game mode: read-back failed ({e})"));
                        return;
                    }
                }
            }
        }
        self.err("The mouse did not take that game mode.");
    }

    fn refresh(&mut self) {
        let mut errors = Vec::new();
        match self.mouse.identity() {
            Ok(id) => {
                self.identity = id;
                self.battery_stale = false;
            }
            Err(e) => {
                self.battery_stale = true;
                errors.push(format!("identity: {e}"));
            }
        }
        match self.mouse.read_config() {
            Ok(config) => {
                self.game_mode = config.game_mode();
                self.config = config;
                self.sync_lod_from_config();
            }
            Err(e) => errors.push(format!("config: {e}")),
        }
        match self.mouse.version() {
            Ok(v) if !v.is_empty() => self.version = v,
            Ok(_) => {}
            Err(e) => errors.push(format!("firmware: {e}")),
        }
        if errors.is_empty() {
            self.ok("Refreshed from the mouse.");
        } else {
            self.err(format!("Refresh incomplete: {}", errors.join("; ")));
        }
    }

    fn backup(&mut self) {
        if self.demo {
            self.ok("Demo mode: backup simulated.");
            return;
        }
        match state::backup_now(&self.config) {
            Ok(path) => self.ok(format!("Backup saved to {}.", path.display())),
            Err(e) => self.err(format!("Backup failed: {e}")),
        }
    }

    fn restore_prompt(&mut self) {
        if self.confirm_restore {
            self.confirm_restore = false;
            self.restore_original();
        } else {
            self.confirm_restore = true;
            self.info("Press Enter again to restore the original config, Esc to cancel.");
        }
    }

    fn restore_original(&mut self) {
        if self.demo {
            self.ok("Demo mode: restore simulated.");
            return;
        }
        let original = match state::load_original() {
            Ok(c) => c,
            Err(e) => {
                self.err(format!("No original backup yet: {e}"));
                return;
            }
        };
        let result = match self.mouse.write_config(&original) {
            Ok(()) => self.mouse.confirm_config(&original),
            Err(e) => Err(e),
        };
        match result {
            Ok(()) => {
                self.ok("Original config restored.");
                self.reload();
            }
            Err(e) => self.err(format!("Restore failed: {e}")),
        }
    }

    fn ok(&mut self, text: impl Into<String>) {
        self.message = Message {
            text: text.into(),
            kind: MsgKind::Ok,
        };
    }

    fn err(&mut self, text: impl Into<String>) {
        self.message = Message {
            text: text.into(),
            kind: MsgKind::Err,
        };
    }

    fn info(&mut self, text: impl Into<String>) {
        self.message = Message {
            text: text.into(),
            kind: MsgKind::Info,
        };
    }

    fn enabled_stages(&self) -> u8 {
        self.config.enabled_stages().clamp(1, DPI_STAGES as u8)
    }

    fn active_stage(&self) -> u8 {
        let raw = if self.identity.connect_mode == 0 {
            self.config.wired_dpi_stage()
        } else {
            self.config.wireless_dpi_stage()
        };
        raw.min(self.enabled_stages() - 1)
    }

    fn active_rate_index(&self) -> u8 {
        if self.identity.connect_mode == 0 {
            self.config.wired_rate_index()
        } else {
            self.config.wireless_rate_index()
        }
    }

    fn items(&self) -> Vec<Item> {
        let mut items = Vec::new();
        let id = &self.identity;

        let low_battery = id.battery <= 20 && id.charging == 0;
        let battery = if self.battery_stale {
            format!("{}  battery unreadable (mouse asleep?)", battery_bar(0, 12))
        } else {
            format!(
                "{}  {}%{}{}",
                battery_bar(id.battery, 12),
                id.battery,
                if id.charging != 0 { "  charging" } else { "" },
                if low_battery {
                    "  low - charge soon"
                } else {
                    ""
                }
            )
        };
        items.push(Item {
            group: "DEVICE",
            row: None,
            label: "Battery".into(),
            value: battery,
        });
        items.push(Item {
            group: "DEVICE",
            row: None,
            label: "Link".into(),
            value: format!(
                "{}  ·  {}",
                link_name(id.connect_mode),
                if id.connected { "online" } else { "offline" }
            ),
        });
        items.push(Item {
            group: "DEVICE",
            row: None,
            label: "Firmware".into(),
            value: if self.version.is_empty() {
                format!("0x{:08x}", id.firmware)
            } else {
                self.version.clone()
            },
        });
        items.push(Item {
            group: "DEVICE",
            row: None,
            label: "Interface".into(),
            value: format!("{}  ({:04x}:{:04x})", self.mouse.path(), id.vid, id.pid),
        });

        let stages = self.enabled_stages();
        let active = self.active_stage();
        items.push(Item {
            group: "PERFORMANCE",
            row: Some(Row::Stage),
            label: "Active DPI stage".into(),
            value: format!(
                "{} / {}   ({} DPI)",
                active + 1,
                stages,
                self.config.dpi(active as usize)
            ),
        });
        for stage in 0..stages {
            let x = self.config.dpi(stage as usize);
            let y = self.config.dpi_y(stage as usize);
            let mut value = if y != 0 && y != x {
                format!("{x} x {y} DPI")
            } else {
                format!("{x} DPI")
            };
            if stage == active {
                value.push_str("  · active");
            }
            items.push(Item {
                group: "PERFORMANCE",
                row: Some(Row::Dpi(stage)),
                label: format!("DPI stage {}", stage + 1),
                value,
            });
        }
        let rate_index = self.active_rate_index();
        items.push(Item {
            group: "PERFORMANCE",
            row: Some(Row::Rate),
            label: "Polling rate".into(),
            value: match self.model.rates.get(rate_index as usize) {
                Some(hz) => format!("{hz} Hz"),
                None => format!("index {rate_index}"),
            },
        });
        items.push(Item {
            group: "PERFORMANCE",
            row: Some(Row::Debounce),
            label: "Debounce".into(),
            value: format!("{} ms", self.config.debounce_ms()),
        });
        items.push(Item {
            group: "PERFORMANCE",
            row: Some(Row::Sleep),
            label: "Sleep".into(),
            value: match self.config.sleep_minutes() {
                0 => "off".into(),
                n => format!("{n} min"),
            },
        });

        let lod = self
            .model
            .lod
            .get(self.lod_index as usize)
            .copied()
            .unwrap_or(0.0);
        items.push(Item {
            group: "SENSOR",
            row: Some(Row::Lod),
            label: "Lift-off distance".into(),
            value: if self.model.lod_in_sensor {
                format!("{lod:.1} mm")
            } else {
                format!("{lod:.1} mm   (write-only)")
            },
        });
        let rot = self.config.rotate_degrees();
        items.push(Item {
            group: "SENSOR",
            row: Some(Row::Rotation),
            label: "Sensor rotation".into(),
            value: if rot == 0 {
                format!("0°   (raw {})", self.config.rotate())
            } else {
                format!("{rot:+}°   (raw {})", self.config.rotate())
            },
        });
        let (ripple, snap, sync) = self.config.perf_flags();
        items.push(Item {
            group: "SENSOR",
            row: Some(Row::Motion),
            label: "Motion sync".into(),
            value: on_off(sync),
        });
        items.push(Item {
            group: "SENSOR",
            row: Some(Row::Ripple),
            label: "Ripple control".into(),
            value: on_off(ripple),
        });
        items.push(Item {
            group: "SENSOR",
            row: Some(Row::Snap),
            label: "Angle snap".into(),
            value: on_off(snap),
        });

        let palette = (0..stages)
            .map(led_color_name)
            .collect::<Vec<_>>()
            .join(" · ");
        items.push(Item {
            group: "STATUS LED",
            row: None,
            label: "DPI flash palette".into(),
            value: format!("{palette}   (fixed by firmware)"),
        });
        items.push(Item {
            group: "STATUS LED",
            row: None,
            label: "DPI flash colour".into(),
            value: format!(
                "{} · stage {} ({} DPI)",
                led_color_name(active),
                active + 1,
                self.config.dpi(active as usize)
            ),
        });
        items.push(Item {
            group: "STATUS LED",
            row: None,
            label: "Steady LED".into(),
            value: "status/charge indicator, not addressable".into(),
        });
        items.push(Item {
            group: "STATUS LED",
            row: Some(Row::LedFollow),
            label: "UI accent follows LED".into(),
            value: on_off(self.accent_follows),
        });
        items.push(Item {
            group: "STATUS LED",
            row: Some(Row::Accent),
            label: "UI accent".into(),
            value: if self.accent_follows {
                format!("{}  (follows DPI flash)", hex_rgb(self.accent_color()))
            } else {
                format!("{}  (custom)", hex_rgb(self.accent_color()))
            },
        });

        for info in buttons::BUTTONS {
            let (kind, value) = self.config.button(info.slot as usize);
            let value = if kind == buttons::KIND_MACRO {
                match self.macro_names[info.slot as usize].as_deref() {
                    Some(name) => format!("Macro · {name}"),
                    None => "Macro".to_string(),
                }
            } else {
                buttons::describe(kind, value)
            };
            items.push(Item {
                group: "BUTTONS",
                row: Some(Row::Button(info.slot)),
                label: format!("{} button", info.name),
                value,
            });
        }

        items.push(Item {
            group: "PROFILE",
            row: Some(Row::Profile),
            label: "Profile".into(),
            value: format!("{} / 3", self.config.profile() + 1),
        });
        items.push(Item {
            group: "PROFILE",
            row: Some(Row::Game),
            label: "Game mode".into(),
            value: format!("{}", self.game_mode),
        });

        items.push(Item {
            group: "TOOLS",
            row: Some(Row::Refresh),
            label: "Refresh".into(),
            value: "re-read everything from the mouse".into(),
        });
        items.push(Item {
            group: "TOOLS",
            row: Some(Row::Backup),
            label: "Backup config".into(),
            value: "save a copy of the current config".into(),
        });
        items.push(Item {
            group: "TOOLS",
            row: Some(Row::Restore),
            label: "Restore original".into(),
            value: "write the first backup back".into(),
        });
        items.push(Item {
            group: "TOOLS",
            row: Some(Row::Config),
            label: "Config block".into(),
            value: "hex dump + diff (d)".into(),
        });
        items.push(Item {
            group: "TOOLS",
            row: Some(Row::Log),
            label: "Frame log".into(),
            value: if self.show_log {
                "shown (f)".into()
            } else {
                "hidden (f)".into()
            },
        });

        items
    }

    fn display_items(&self, width: usize, label_w: usize) -> (Vec<String>, Option<usize>) {
        let items = self.items();
        let mut lines = Vec::new();
        let mut last_group = "";
        let mut selectable = 0usize;
        let mut selected_line = None;

        for item in &items {
            if item.group != last_group {
                lines.push(style(
                    &fit(&format!(" {}", item.group), width),
                    &self.accent_code(),
                    self.color,
                ));
                last_group = item.group;
            }
            match item.row {
                Some(_) => {
                    let selected = selectable == self.selected;
                    let marker = if selected { "> " } else { "  " };
                    let label = fit(&item.label, label_w);
                    let text = fit(&format!("{marker}{label}  {}", item.value), width);
                    let item_color = match item.row {
                        Some(Row::Stage) => Some(stage_color(self.active_stage())),
                        Some(Row::Dpi(stage)) => Some(stage_color(stage)),
                        _ => None,
                    };
                    if selected {
                        selected_line = Some(lines.len());
                        let code = match item_color {
                            Some(c) => format!("\x1b[7;38;2;{};{};{}m", c[0], c[1], c[2]),
                            None => "\x1b[7m".to_string(),
                        };
                        lines.push(style(&text, &code, self.color));
                    } else if let Some(c) = item_color {
                        lines.push(style(&text, &rgb_code(c, false), self.color));
                    } else {
                        lines.push(text);
                    }
                    selectable += 1;
                }
                None => {
                    let text = fit(
                        &format!(
                            "   {:<label_w$}  {}",
                            item.label,
                            item.value,
                            label_w = label_w
                        ),
                        width,
                    );
                    if item.group == "STATUS LED" && item.label == "DPI flash colour" {
                        lines.push(style(
                            &text,
                            &rgb_code(stage_color(self.active_stage()), false),
                            self.color,
                        ));
                    } else {
                        lines.push(style(&text, "\x1b[2m", self.color));
                    }
                }
            }
        }
        (lines, selected_line)
    }

    fn macro_view(&self, edit: &MacroEdit, width: usize, height: usize) -> Vec<String> {
        let mut lines = Vec::new();
        if height == 0 {
            return lines;
        }
        let name = buttons::button_info(edit.slot)
            .map(|b| b.name)
            .unwrap_or("?");
        lines.push(style(
            &fit(
                &format!(" MACRO · {} BUTTON", name.to_ascii_uppercase()),
                width,
            ),
            &self.accent_code(),
            self.color,
        ));
        if lines.len() < height {
            lines.push(style(&"─".repeat(width), "\x1b[2m", self.color));
        }

        let sequence = if edit.buffer.is_empty() {
            "Sequence: _".to_string()
        } else {
            format!("Sequence: {}_", edit.buffer)
        };
        for chunk in wrap(&sequence, width.saturating_sub(2)) {
            if lines.len() >= height {
                break;
            }
            lines.push(fit(&format!(" {chunk}"), width));
        }

        let (status, code) = match macros::parse(&edit.buffer) {
            Ok(events) => (macros::summary(&events), "\x1b[32m"),
            Err(error) => (error, "\x1b[31m"),
        };
        if lines.len() < height {
            lines.push(style(&fit(&format!(" {status}"), width), code, self.color));
        }
        if let Some(error) = &edit.error {
            if macros::parse(&edit.buffer).is_ok() && lines.len() < height {
                lines.push(style(
                    &fit(&format!(" {error}"), width),
                    "\x1b[31m",
                    self.color,
                ));
            }
        }
        if let Some(existing) = self.macro_names[edit.slot as usize].as_deref() {
            if lines.len() < height {
                lines.push(style(
                    &fit(&format!(" Existing on mouse: {existing}"), width),
                    "\x1b[2m",
                    self.color,
                ));
            }
        }
        if lines.len() < height {
            lines.push(fit("", width));
        }
        for help in [
            " Syntax:",
            "   ctrl+c   shift+f10   alt+tab   enter   f5",
            "   delay:50   mouse:left   mouse:right   wheel:up",
            "   \"Hello\" types text; spaces separate steps",
            "   + joins modifiers; 13 events (52 bytes) max",
            "   A key is one down + one up; a chord is four events.",
        ] {
            if lines.len() >= height {
                break;
            }
            lines.push(style(&fit(help, width), "\x1b[2m", self.color));
        }
        while lines.len() < height {
            lines.push(fit("", width));
        }
        lines.truncate(height);
        lines
    }

    fn remap_view(&self, remap: &Remap, width: usize, height: usize) -> Vec<String> {
        let actions = buttons::filter(&remap.query);
        let mut lines = Vec::new();
        if height == 0 {
            return lines;
        }

        let name = buttons::button_info(remap.slot)
            .map(|b| b.name)
            .unwrap_or("?");
        lines.push(style(
            &fit(
                &format!(" REMAP {} BUTTON", name.to_ascii_uppercase()),
                width,
            ),
            "\x1b[1;36m",
            self.color,
        ));
        if lines.len() < height {
            let filter = if remap.query.is_empty() {
                "type to filter".to_string()
            } else {
                format!("{}_", remap.query)
            };
            lines.push(fit(&format!(" Filter: {filter}"), width));
        }
        if lines.len() < height {
            lines.push(style(&"─".repeat(width), "\x1b[2m", self.color));
        }

        let visible = height.saturating_sub(lines.len());
        let start = remap
            .selected
            .saturating_sub(visible / 2)
            .min(actions.len().saturating_sub(visible));
        for (i, action) in actions.iter().enumerate().skip(start).take(visible) {
            let marker = if i == remap.selected { "> " } else { "  " };
            let text = fit(
                &format!("{marker}{:<8}  {}", action.group, action.name),
                width,
            );
            if i == remap.selected {
                lines.push(style(&text, "\x1b[7m", self.color));
            } else {
                lines.push(text);
            }
        }
        while lines.len() < height {
            lines.push(fit("", width));
        }
        lines.truncate(height);
        lines
    }

    fn config_view(&self, width: usize, height: usize) -> Vec<String> {
        let mut lines = Vec::new();
        if height == 0 {
            return lines;
        }
        lines.push(style(
            &fit(" CONFIG BLOCK (live; * differs from original)", width),
            &self.accent_code(),
            self.color,
        ));
        let original = if self.demo {
            None
        } else {
            state::load_original().ok()
        };
        let changed: Vec<(usize, u8, u8)> = match &original {
            Some(orig) => (0..63)
                .filter(|&i| self.config.body[i] != orig.body[i])
                .map(|i| (i, self.config.body[i], orig.body[i]))
                .collect(),
            None => Vec::new(),
        };
        for row in 0..8 {
            let start = row * 8;
            if start >= 63 {
                break;
            }
            let end = (start + 8).min(63);
            let bytes = self.config.body[start..end]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join(" ");
            let mark = if changed.iter().any(|(i, _, _)| *i >= start && *i < end) {
                "*"
            } else {
                " "
            };
            lines.push(fit(&format!("{mark}{start:02}  {bytes}"), width));
            if lines.len() >= height {
                return lines;
            }
        }
        if lines.len() < height {
            lines.push(style(&"─".repeat(width), "\x1b[2m", self.color));
        }
        if original.is_some() {
            if changed.is_empty() {
                if lines.len() < height {
                    lines.push(fit(" No differences from the original backup.", width));
                }
            } else {
                for chunk in changed.chunks(4) {
                    if lines.len() >= height {
                        break;
                    }
                    let parts = chunk
                        .iter()
                        .map(|(i, now, was)| format!("[{i:02}] {now:02x} (was {was:02x})"))
                        .collect::<Vec<_>>()
                        .join("  ");
                    lines.push(fit(&format!(" {parts}"), width));
                }
            }
        } else if lines.len() < height {
            lines.push(fit(
                " No original backup yet; it is saved before the first write.",
                width,
            ));
        }
        while lines.len() < height {
            lines.push(fit("", width));
        }
        lines.truncate(height);
        lines
    }

    pub fn render(&self, width: usize, height: usize) -> String {
        let w = width.max(40);
        let h = height.max(12);
        let label_w = if w >= 80 { 26 } else { 18 };

        let available = h.saturating_sub(5);
        let log_total = if self.show_log
            && self.remap.is_none()
            && self.macro_edit.is_none()
            && !self.show_config
        {
            (available.saturating_sub(3)).min((h / 4).clamp(2, 7) + 1)
        } else {
            0
        };
        let main_h = available.saturating_sub(log_total).max(1);

        let main_lines = if let Some(edit) = &self.macro_edit {
            self.macro_view(edit, w, main_h)
        } else if let Some(remap) = &self.remap {
            self.remap_view(remap, w, main_h)
        } else if self.show_config {
            self.config_view(w, main_h)
        } else {
            let (content, selected_line) = self.display_items(w, label_w);
            let start = if content.len() <= main_h {
                0
            } else {
                let sel = selected_line.unwrap_or(0);
                let start = if sel >= main_h { sel + 1 - main_h } else { 0 };
                start.min(content.len() - main_h)
            };
            let end = (start + main_h).min(content.len());
            let mut out = content[start..end].to_vec();
            for _ in (end - start)..main_h {
                out.push(fit("", w));
            }
            out
        };

        let mut lines = Vec::new();
        let left = format!(" MCHOSE TUI {}  ·  {}", VERSION, self.mouse.name());
        let battery = if self.battery_stale {
            "--".to_string()
        } else {
            format!("{}%", self.identity.battery)
        };
        let low_battery =
            !self.battery_stale && self.identity.battery <= 20 && self.identity.charging == 0;
        let right = format!(
            "{}  ·  {}{}{} ",
            link_name(self.identity.connect_mode),
            battery,
            if self.identity.charging != 0 {
                " charging"
            } else {
                ""
            },
            if low_battery { " LOW" } else { "" }
        );
        lines.push(style(
            &join_lr(&left, &right, w),
            &self.accent_code(),
            self.color,
        ));
        lines.push(style(&"─".repeat(w), "\x1b[2m", self.color));

        for line in &main_lines {
            lines.push(line.clone());
        }

        if self.show_log {
            lines.push(style(&fit(" FRAMES", w), &self.accent_code(), self.color));
            let entries = log_total.saturating_sub(1);
            let logs = proto::log_snapshot();
            let skip = logs.len().saturating_sub(entries);
            let mut shown = 0;
            for line in logs.iter().skip(skip).take(entries) {
                lines.push(style(&fit(&format!("  {line}"), w), "\x1b[2m", self.color));
                shown += 1;
            }
            if shown == 0 {
                lines.push(style(
                    &fit("  (demo mode: no HID frames)", w),
                    "\x1b[2m",
                    self.color,
                ));
                shown = 1;
            }
            for _ in shown..entries {
                lines.push(fit("", w));
            }
        }

        lines.push(style(&"─".repeat(w), "\x1b[2m", self.color));
        let (status, code) = if let Some(edit) = &self.macro_edit {
            let name = buttons::button_info(edit.slot)
                .map(|b| b.name)
                .unwrap_or("?");
            let (detail, code) = if let Some(error) = &edit.error {
                (error.clone(), "\x1b[31m")
            } else {
                match macros::parse(&edit.buffer) {
                    Ok(events) => (macros::summary(&events), "\x1b[33m"),
                    Err(error) => (error, "\x1b[31m"),
                }
            };
            (
                format!(" Macro for {name}: {detail}   (Enter save, Esc cancel)"),
                code,
            )
        } else if let Some(remap) = &self.remap {
            let name = buttons::button_info(remap.slot)
                .map(|b| b.name)
                .unwrap_or("?");
            let filter = if remap.query.is_empty() {
                "(all)".to_string()
            } else {
                remap.query.clone()
            };
            (
                format!(
                    " Remap {name}: filter {filter}  ·  {} matches  ·  Enter apply, Esc cancel",
                    buttons::filter(&remap.query).len()
                ),
                "\x1b[33m",
            )
        } else if let Some(edit) = &self.edit {
            let (what, value) = match edit.target {
                EditTarget::Dpi(stage) => (format!("DPI stage {}", stage + 1), edit.buffer.clone()),
                EditTarget::Rotation => {
                    ("sensor rotation".to_string(), format!("{}°", edit.buffer))
                }
                EditTarget::Accent => ("UI accent".to_string(), edit.buffer.clone()),
            };
            (
                format!(" Editing {what}: {value}_   (Enter apply, Esc cancel)"),
                "\x1b[33m",
            )
        } else if self.show_config {
            (
                " Config block: d or Esc to return, r to refresh".to_string(),
                "\x1b[33m",
            )
        } else if self.confirm_restore {
            (
                " Press Enter again to restore the original config, Esc to cancel".to_string(),
                "\x1b[31m",
            )
        } else {
            (
                format!(" {}", self.message.text),
                match self.message.kind {
                    MsgKind::Ok => "\x1b[32m",
                    MsgKind::Err => "\x1b[31m",
                    MsgKind::Info => "\x1b[33m",
                },
            )
        };
        lines.push(style(&fit(&status, w), code, self.color));
        let keys = if self.macro_edit.is_some() {
            " Type a sequence  Backspace delete  Enter save  Esc cancel"
        } else if self.remap.is_some() {
            " Type to filter  Up/Down move  PgUp/PgDn jump  Enter apply  Esc cancel"
        } else if self.show_config {
            " d or Esc back  r refresh  q quit"
        } else {
            " Up/Down move  Left/Right adjust  Enter edit  d config  r refresh  q quit"
        };
        lines.push(style(&fit(keys, w), "\x1b[2m", self.color));
        lines.join("\r\n")
    }
}

pub fn render_error(message: &str, width: usize, height: usize, color: bool) -> String {
    let w = width.max(40);
    let h = height.max(10);
    let mut lines = Vec::new();
    lines.push(style(
        &fit(" MCHOSE TUI  ·  startup problem", w),
        "\x1b[1;36m",
        color,
    ));
    lines.push(style(&"─".repeat(w), "\x1b[2m", color));

    let max = h.saturating_sub(4);
    let mut count = 0;
    'outer: for raw in message.lines() {
        for chunk in wrap(raw, w.saturating_sub(2)) {
            if count >= max {
                break 'outer;
            }
            lines.push(fit(&format!(" {}", chunk), w));
            count += 1;
        }
    }
    while count < max {
        lines.push(fit("", w));
        count += 1;
    }
    lines.push(style(&"─".repeat(w), "\x1b[2m", color));
    lines.push(style(&fit(" r rescan    q quit", w), "\x1b[2m", color));
    lines.join("\r\n")
}

fn fit(s: &str, width: usize) -> String {
    let mut out = String::with_capacity(width);
    let mut n = 0;
    for ch in s.chars() {
        if n >= width {
            break;
        }
        out.push(ch);
        n += 1;
    }
    while n < width {
        out.push(' ');
        n += 1;
    }
    out
}

fn style(s: &str, code: &str, color: bool) -> String {
    if color {
        format!("{code}{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn join_lr(left: &str, right: &str, width: usize) -> String {
    let rw = right.chars().count();
    if rw >= width {
        return fit(right, width);
    }
    let lw = width - rw;
    format!("{}{}", fit(left, lw), right)
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    for raw in text.lines() {
        if raw.trim().is_empty() {
            out.push(String::new());
            continue;
        }
        let indent = raw.len() - raw.trim_start().len();
        let indent = indent.min(width.saturating_sub(1));
        let prefix = " ".repeat(indent);
        let avail = width.saturating_sub(indent).max(1);
        let mut line = String::new();
        let mut len = 0usize;
        for word in raw.split_whitespace() {
            let word_len = word.chars().count();
            if len == 0 {
                if word_len > avail {
                    let mut chunk = String::new();
                    let mut n = 0usize;
                    for ch in word.chars() {
                        chunk.push(ch);
                        n += 1;
                        if n == avail {
                            out.push(format!("{prefix}{chunk}"));
                            chunk.clear();
                            n = 0;
                        }
                    }
                    line = chunk;
                    len = n;
                } else {
                    line.push_str(word);
                    len = word_len;
                }
            } else if len + 1 + word_len > avail {
                out.push(format!("{prefix}{line}"));
                line = word.to_string();
                len = word_len;
            } else {
                line.push(' ');
                line.push_str(word);
                len += 1 + word_len;
            }
        }
        out.push(format!("{prefix}{line}"));
    }
    out
}

/// Parse `#RRGGBB` or `RRGGBB` into an RGB triple.
pub fn parse_hex_color(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some([r, g, b])
}

fn hex_rgb(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

fn rgb_code(c: [u8; 3], bold: bool) -> String {
    if bold {
        format!("\x1b[1;38;2;{};{};{}m", c[0], c[1], c[2])
    } else {
        format!("\x1b[38;2;{};{};{}m", c[0], c[1], c[2])
    }
}

/// The L7 manual's fixed DPI indicator palette. The firmware chooses the
/// colour from the stage index; it is not an addressable RGB setting.
fn stage_color(stage: u8) -> [u8; 3] {
    match stage {
        0 => [255, 70, 70],
        1 => [80, 140, 255],
        2 => [70, 220, 100],
        3 => [255, 220, 60],
        4 => [60, 220, 220],
        _ => [190, 110, 255],
    }
}

fn led_color_name(stage: u8) -> &'static str {
    match stage {
        0 => "red",
        1 => "blue",
        2 => "green",
        3 => "yellow",
        4 => "cyan",
        _ => "purple",
    }
}

fn on_off(value: bool) -> String {
    if value {
        "on".into()
    } else {
        "off".into()
    }
}

/// A button counts as left click when it is the left button's native action or
/// any button explicitly mapped to `mouse.left`.
fn left_click_count(config: &Config) -> usize {
    (0..buttons::BUTTONS.len())
        .filter(|&slot| {
            let (kind, value) = config.button(slot);
            (slot == 0 && kind == buttons::KIND_NATIVE && value == 0)
                || (kind == buttons::KIND_MOUSE && value == buttons::MOUSE_LEFT)
        })
        .count()
}

fn link_name(mode: u8) -> &'static str {
    match mode {
        0 => "wired",
        1 => "2.4 GHz",
        2 => "Bluetooth",
        _ => "unknown link",
    }
}

fn battery_bar(percent: u8, width: usize) -> String {
    let pct = percent.min(100) as usize;
    let filled = (pct * width).div_ceil(100);
    format!(
        "{}{}",
        "█".repeat(filled),
        "░".repeat(width.saturating_sub(filled))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native_config() -> Config {
        let mut c = Config { body: [0u8; 63] };
        for slot in 0..buttons::BUTTONS.len() {
            c.set_button(slot, buttons::KIND_NATIVE, 0);
        }
        c
    }

    #[test]
    fn the_last_left_click_cannot_be_removed() {
        let mut c = native_config();
        assert_eq!(left_click_count(&c), 1);

        // Mapping another button to left click makes the original expendable.
        c.set_button(1, buttons::KIND_MOUSE, buttons::MOUSE_LEFT);
        assert_eq!(left_click_count(&c), 2);
        c.set_button(0, buttons::KIND_KEYBOARD, 0x000400);
        assert_eq!(left_click_count(&c), 1);

        // Taking the only remaining left click away leaves none.
        c.set_button(1, buttons::KIND_MOUSE, 0x020000);
        assert_eq!(left_click_count(&c), 0);
    }

    #[test]
    fn hex_colour_parsing_is_strict() {
        assert_eq!(parse_hex_color("#00C8DC"), Some([0, 200, 220]));
        assert_eq!(parse_hex_color("00c8dc"), Some([0, 200, 220]));
        assert_eq!(parse_hex_color(" #FF0000 "), Some([255, 0, 0]));
        assert_eq!(parse_hex_color("#12345"), None);
        assert_eq!(parse_hex_color("#gggggg"), None);
        assert_eq!(parse_hex_color(""), None);
        assert_eq!(parse_hex_color("#1234567"), None);
    }

    #[test]
    fn led_palette_has_six_distinct_named_colours() {
        let names: Vec<_> = (0..6).map(led_color_name).collect();
        assert_eq!(names, ["red", "blue", "green", "yellow", "cyan", "purple"]);
        let mut seen = Vec::new();
        for stage in 0..6 {
            let c = stage_color(stage);
            assert!(!seen.contains(&c), "stage {stage} repeats a colour");
            seen.push(c);
        }
        // Out-of-range stages fall back to the last (purple) entry.
        assert_eq!(stage_color(9), stage_color(5));
        assert_eq!(led_color_name(9), "purple");
    }

    fn demo_app(accent: Option<[u8; 3]>) -> App {
        App::new(Box::new(DemoMouse::new()), true, true, accent).expect("demo app")
    }

    #[test]
    fn accent_defaults_to_the_led_and_accepts_an_override() {
        let mut app = demo_app(None);
        assert!(app.accent_follows);
        assert_eq!(app.accent_color(), stage_color(app.active_stage()));

        app.toggle_accent_follow();
        assert!(!app.accent_follows);
        assert_eq!(app.accent_color(), DEFAULT_ACCENT);

        let mut app = demo_app(Some([0x12, 0x34, 0x56]));
        assert!(!app.accent_follows);
        assert_eq!(app.accent_color(), [0x12, 0x34, 0x56]);

        app.apply_accent([0, 200, 220]);
        assert_eq!(app.accent_color(), [0, 200, 220]);
        assert!(app.accent_code().contains("38;2;0;200;220"));
    }

    #[test]
    fn rotation_round_trips_through_the_demo_protocol() {
        let mut app = demo_app(None);
        app.apply_rotation(-12);
        assert_eq!(app.config.rotate(), 244);
        assert_eq!(app.config.rotate_degrees(), -12);

        // Out-of-range values are clamped before they reach the wire.
        app.apply_rotation(99);
        assert_eq!(app.config.rotate_degrees(), 30);

        app.apply_rotation(0);
        assert_eq!(app.config.rotate(), 0);
    }

    #[test]
    fn demo_macro_editor_writes_and_binds_a_macro() {
        let mut app = demo_app(None);
        app.open_macro_editor(1);
        assert!(app.macro_edit.is_some());

        let edit = app.macro_edit.as_mut().unwrap();
        edit.buffer = "ctrl+c".into();
        app.handle_key(Key::Enter);

        assert!(app.macro_edit.is_none());
        assert_eq!(app.config.button(1).0, buttons::KIND_MACRO);
        assert_eq!(app.config.button(1).1, 0);
        assert_eq!(app.macro_names[1].as_deref(), Some("TUI ctrl+c"));
        assert_eq!(app.macro_expr[1].as_deref(), Some("ctrl+c"));

        let screen = app.render(100, 40);
        assert!(screen.contains("Macro · TUI ctrl+c"));
    }

    #[test]
    fn the_remap_picker_opens_the_macro_editor() {
        let mut app = demo_app(None);
        app.open_remap(1);
        for ch in "macro".chars() {
            app.handle_key(Key::Char(ch));
        }
        assert!(app
            .remap
            .as_ref()
            .is_some_and(|remap| buttons::filter(&remap.query)
                .iter()
                .any(|action| action.kind == buttons::KIND_MACRO)));
        app.handle_key(Key::Enter);
        assert!(app.remap.is_none());
        assert!(app.macro_edit.is_some());
    }

    #[test]
    fn a_macro_cannot_remove_the_last_left_click() {
        let mut app = demo_app(None);
        app.open_macro_editor(0);
        app.macro_edit.as_mut().unwrap().buffer = "a".into();
        app.handle_key(Key::Enter);

        // The editor stays open with an explanation, and the mapping is intact.
        assert!(app.macro_edit.is_some());
        assert_eq!(app.config.button(0), (buttons::KIND_NATIVE, 0));
        assert!(app
            .macro_edit
            .as_ref()
            .unwrap()
            .error
            .as_deref()
            .unwrap()
            .contains("last left click"));
    }

    #[test]
    fn a_bad_macro_keeps_the_editor_open() {
        let mut app = demo_app(None);
        app.open_macro_editor(1);
        app.macro_edit.as_mut().unwrap().buffer = "nope".into();
        app.handle_key(Key::Enter);

        assert!(app.macro_edit.is_some());
        assert_eq!(app.config.button(1), (buttons::KIND_NATIVE, 0));
        assert!(app
            .macro_edit
            .as_ref()
            .unwrap()
            .error
            .as_deref()
            .unwrap()
            .contains("Unknown key"));
    }

    #[test]
    fn the_macro_editor_shows_syntax_and_a_live_count() {
        let mut app = demo_app(None);
        app.open_macro_editor(1);
        app.macro_edit.as_mut().unwrap().buffer = "ctrl+c".into();
        let screen = app.render(100, 40);
        assert!(screen.contains("MACRO · MIDDLE BUTTON"));
        assert!(screen.contains("4 events · 16/52 bytes"));
        assert!(screen.contains("Syntax:"));
        assert!(screen.contains("delay:50"));
    }

    #[test]
    fn config_view_dumps_the_whole_block() {
        let mut app = demo_app(None);
        app.show_config = true;
        let screen = app.render(100, 40);
        assert!(screen.contains("CONFIG BLOCK"));
        for start in [0, 8, 16, 24, 32, 40, 48, 56] {
            assert!(
                screen.contains(&format!("{start:02}  ")),
                "missing row starting at {start}"
            );
        }
        assert!(screen.contains("Config block: d or Esc"));
    }

    #[test]
    fn the_cli_dump_lists_every_section() {
        let opts = Options {
            demo: true,
            device: None,
            color: false,
            accent: None,
        };
        let text = dump(&opts).unwrap();
        for needle in [
            "MCHOSE L7 Ultra (demo)",
            "Interface",
            "PERFORMANCE",
            "Active DPI stage",
            "DPI stage 1",
            "400 DPI",
            "Polling rate",
            "SENSOR",
            "Lift-off distance",
            "BUTTONS",
            "Left button",
            "PROFILE",
            "Game mode",
            "RAW CONFIG",
        ] {
            assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
        }
        let raw = text.lines().last().unwrap();
        assert_eq!(raw.split_whitespace().count(), 63);
    }
}

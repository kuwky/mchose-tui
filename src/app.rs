//! Application state, key handling and the renderer.

use crate::mouse::{model_info, DemoMouse, ModelInfo, Mouse, RealMouse};
use crate::proto::{self, Config, Identity, DPI_STAGES};
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
    Lod,
    Debounce,
    Sleep,
    Motion,
    Ripple,
    Snap,
    Profile,
    Game,
    Backup,
    Restore,
    Log,
    Refresh,
}

struct Item {
    group: &'static str,
    row: Option<Row>,
    label: String,
    value: String,
}

struct Edit {
    stage: u8,
    buffer: String,
}

pub struct Options {
    pub demo: bool,
    pub device: Option<String>,
    pub color: bool,
}

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
    message: Message,
    confirm_restore: bool,
    show_log: bool,
    color: bool,
    demo: bool,
    last_poll: Instant,
    battery_stale: bool,
}

pub fn open(opts: &Options) -> Result<App, String> {
    let mouse: Box<dyn Mouse> = if opts.demo {
        Box::new(DemoMouse::new())
    } else {
        Box::new(RealMouse::discover(opts.device.as_deref())?)
    };
    App::new(mouse, opts.demo, opts.color).map_err(|e| {
        format!(
            "The interface was found but the mouse did not answer.\n\n{e}\n\n\
             Move the mouse to wake it, then press r to retry."
        )
    })
}

impl App {
    fn new(mut mouse: Box<dyn Mouse>, demo: bool, color: bool) -> io::Result<Self> {
        let identity = mouse.identity()?;
        let model = model_info(identity.pid, mouse.name());
        let version = mouse.version().unwrap_or_default();
        let config = mouse.read_config()?;
        let game_mode = mouse.game_mode().unwrap_or(0);
        let lod_index = if demo {
            model.default_lod
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
            message: Message::default(),
            confirm_restore: false,
            show_log: false,
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
        if self.edit.is_some() {
            self.handle_edit(key);
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
            Key::Enter => match edit.buffer.parse::<u16>() {
                Ok(value) => self.apply_dpi(edit.stage, value),
                Err(_) => self.err("DPI must be a number."),
            },
            Key::Esc => self.info("Edit cancelled."),
            Key::Backspace => {
                edit.buffer.pop();
                self.edit = Some(edit);
            }
            Key::Char(c) if c.is_ascii_digit() => {
                if edit.buffer.len() < 5 {
                    edit.buffer.push(c);
                }
                self.edit = Some(edit);
            }
            _ => self.edit = Some(edit),
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
            Row::Dpi(stage) => self.start_edit(stage),
            Row::Backup => self.backup(),
            Row::Restore => self.restore_prompt(),
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
            Row::Motion | Row::Ripple | Row::Snap => self.toggle(row),
            _ => {}
        }
    }

    fn start_edit(&mut self, stage: u8) {
        self.edit = Some(Edit {
            stage,
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
            self.config = config;
        }
        if let Ok(game) = self.mouse.game_mode() {
            self.game_mode = game;
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
                if lod != self.lod_index {
                    self.lod_index = lod;
                    if !self.demo {
                        state::save_lod(self.identity.pid, lod);
                    }
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
            match self.mouse.game_mode() {
                Ok(got) if got == mode => {
                    self.game_mode = mode;
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
            Ok(config) => self.config = config,
            Err(e) => errors.push(format!("config: {e}")),
        }
        match self.mouse.game_mode() {
            Ok(game) => self.game_mode = game,
            Err(e) => errors.push(format!("game mode: {e}")),
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

        let battery = if self.battery_stale {
            format!("{}  battery unreadable (mouse asleep?)", battery_bar(0, 12))
        } else {
            format!(
                "{}  {}%{}",
                battery_bar(id.battery, 12),
                id.battery,
                if id.charging != 0 { "  charging" } else { "" }
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
            value: format!("{lod:.1} mm   (write-only)"),
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
                    "\x1b[1;36m",
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
                    if selected {
                        selected_line = Some(lines.len());
                        lines.push(style(&text, "\x1b[7m", self.color));
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
                    lines.push(style(&text, "\x1b[2m", self.color));
                }
            }
        }
        (lines, selected_line)
    }

    pub fn render(&self, width: usize, height: usize) -> String {
        let w = width.max(40);
        let h = height.max(12);
        let label_w = if w >= 80 { 26 } else { 18 };
        let (content, selected_line) = self.display_items(w, label_w);

        let available = h.saturating_sub(5);
        let log_total = if self.show_log {
            (available.saturating_sub(3)).min((h / 4).clamp(2, 7) + 1)
        } else {
            0
        };
        let main_h = available.saturating_sub(log_total).max(1);
        let start = if content.len() <= main_h {
            0
        } else {
            let sel = selected_line.unwrap_or(0);
            let start = if sel >= main_h { sel + 1 - main_h } else { 0 };
            start.min(content.len() - main_h)
        };
        let end = (start + main_h).min(content.len());

        let mut lines = Vec::new();
        let left = format!(" MCHOSE TUI {}  ·  {}", VERSION, self.mouse.name());
        let battery = if self.battery_stale {
            "--".to_string()
        } else {
            format!("{}%", self.identity.battery)
        };
        let right = format!(
            "{}  ·  {}{} ",
            link_name(self.identity.connect_mode),
            battery,
            if self.identity.charging != 0 {
                " charging"
            } else {
                ""
            }
        );
        lines.push(style(&join_lr(&left, &right, w), "\x1b[1;36m", self.color));
        lines.push(style(&"─".repeat(w), "\x1b[2m", self.color));

        for line in &content[start..end] {
            lines.push(line.clone());
        }
        for _ in (end - start)..main_h {
            lines.push(fit("", w));
        }

        if self.show_log {
            lines.push(style(&fit(" FRAMES", w), "\x1b[1;36m", self.color));
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
        let (status, code) = if let Some(edit) = &self.edit {
            (
                format!(
                    " Editing DPI stage {}: {}_   (Enter apply, Esc cancel)",
                    edit.stage + 1,
                    edit.buffer
                ),
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
        lines.push(style(
            &fit(
                " Up/Down move  Left/Right adjust  Enter apply  r refresh  b backup  q quit",
                w,
            ),
            "\x1b[2m",
            self.color,
        ));
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

fn on_off(value: bool) -> String {
    if value {
        "on".into()
    } else {
        "off".into()
    }
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

//! Device model table and the two backends: real hidraw and an in-memory demo.

use crate::hid::{self, HidRaw, Node};
use crate::proto::{self, Config, Identity};
use std::io;
use std::path::PathBuf;

#[derive(Clone)]
pub struct ModelInfo {
    pub dpi_max: u16,
    pub lod: Vec<f32>,
    pub rates: Vec<u32>,
    pub default_lod: u8,
}

/// Model quirks. The protocol is shared, but the DPI ceiling, the LOD list and
/// the polling-rate list are per model. Unknown models get the safe defaults.
pub fn model_info(pid: u16, name: &str) -> ModelInfo {
    let lower = name.to_ascii_lowercase();
    let is_42000 = pid == 0x00b1 || lower.contains("ultra");
    let dpi_max = if is_42000 { 42000 } else { 26000 };
    let (lod, default_lod) = if is_42000 {
        (vec![0.7, 1.0, 2.0], 1)
    } else {
        (vec![1.0, 2.0], 0)
    };
    let rates = if lower.contains("1k") || lower.contains("1 k") {
        vec![125, 500, 1000]
    } else {
        vec![125, 500, 1000, 2000, 4000, 8000]
    };
    ModelInfo {
        dpi_max,
        lod,
        rates,
        default_lod,
    }
}

pub trait Mouse {
    fn name(&self) -> &str;
    fn path(&self) -> &str;
    fn identity(&mut self) -> io::Result<Identity>;
    fn version(&mut self) -> io::Result<String>;
    fn read_config(&mut self) -> io::Result<Config>;
    fn write_config(&mut self, config: &Config) -> io::Result<()>;
    fn confirm_config(&mut self, config: &Config) -> io::Result<()>;
    fn set_rate(&mut self, index: u8) -> io::Result<()>;
    fn set_flags(
        &mut self,
        lod: u8,
        ripple: bool,
        angle_snap: bool,
        motion_sync: bool,
        rotate: u8,
    ) -> io::Result<()>;
    fn set_profile(&mut self, profile: u8) -> io::Result<()>;
    fn set_game_mode(&mut self, mode: u8) -> io::Result<()>;
    fn game_mode(&mut self) -> io::Result<u8>;
}

pub struct RealMouse {
    dev: HidRaw,
    name: String,
    path: String,
}

impl RealMouse {
    pub fn discover(explicit: Option<&str>) -> Result<Self, String> {
        let all = hid::nodes().map_err(|e| format!("cannot scan /sys/class/hidraw: {e}"))?;

        if let Some(path) = explicit {
            let wanted = PathBuf::from(path);
            let node = all
                .iter()
                .find(|n| n.path == wanted)
                .cloned()
                .unwrap_or(Node {
                    path: wanted,
                    vid: 0,
                    pid: 0,
                    name: "explicit device".into(),
                    descriptor: Vec::new(),
                });
            return Self::open_node(node).map_err(|e| {
                format!(
                    "cannot use {path}: {e}\n\n\
                     If this is a permission problem, install the udev rule:\n  \
                     sudo install -m644 70-mchose.rules /etc/udev/rules.d/70-mchose.rules\n  \
                     sudo udevadm control --reload-rules && sudo udevadm trigger"
                )
            });
        }

        let mut candidates: Vec<Node> = all.iter().filter(|n| n.is_config()).cloned().collect();
        candidates.sort_by_key(|n| !n.name.to_ascii_uppercase().contains("MCHOSE"));

        let mut failures = Vec::new();
        for node in candidates {
            match Self::open_node(node.clone()) {
                Ok(mouse) => return Ok(mouse),
                Err(e) => failures.push((node, e.to_string())),
            }
        }
        Err(discovery_error(&all, &failures))
    }

    fn open_node(node: Node) -> io::Result<Self> {
        let dev = HidRaw::open(&node.path)
            .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", node.path.display())))?;
        let id = proto::identity(&dev)
            .map_err(|e| io::Error::new(e.kind(), format!("no protocol reply: {e}")))?;
        if id.connect_mode == 1 && !id.connected {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "receiver is idle (no mouse connected)",
            ));
        }
        Ok(Self {
            dev,
            name: hid::clean_name(&node.name),
            path: node.path.to_string_lossy().to_string(),
        })
    }
}

impl Mouse for RealMouse {
    fn name(&self) -> &str {
        &self.name
    }

    fn path(&self) -> &str {
        &self.path
    }

    fn identity(&mut self) -> io::Result<Identity> {
        proto::identity(&self.dev)
    }

    fn version(&mut self) -> io::Result<String> {
        proto::version_string(&self.dev)
    }

    fn read_config(&mut self) -> io::Result<Config> {
        Config::read(&self.dev)
    }

    fn write_config(&mut self, config: &Config) -> io::Result<()> {
        config.write(&self.dev)
    }

    fn confirm_config(&mut self, config: &Config) -> io::Result<()> {
        proto::confirm(&self.dev, config).map(|_| ())
    }

    fn set_rate(&mut self, index: u8) -> io::Result<()> {
        proto::set_report_rate(&self.dev, index, index)
    }

    fn set_flags(
        &mut self,
        lod: u8,
        ripple: bool,
        angle_snap: bool,
        motion_sync: bool,
        rotate: u8,
    ) -> io::Result<()> {
        proto::set_flags(&self.dev, lod, ripple, angle_snap, motion_sync, rotate)
    }

    fn set_profile(&mut self, profile: u8) -> io::Result<()> {
        proto::set_profile(&self.dev, profile)
    }

    fn set_game_mode(&mut self, mode: u8) -> io::Result<()> {
        proto::set_game_mode(&self.dev, mode)
    }

    fn game_mode(&mut self) -> io::Result<u8> {
        proto::basics(&self.dev).map(|(_, _, _, game)| game)
    }
}

fn discovery_error(all: &[Node], failures: &[(Node, String)]) -> String {
    let mut msg = String::from("No MCHOSE mouse answered.\n");
    let mchose: Vec<&Node> = all.iter().filter(|n| n.is_mchose()).collect();

    if failures.is_empty() {
        if mchose.is_empty() {
            msg.push_str(
                "\nNo MCHOSE hidraw node found (vendor 5253 or 3837).\n\
                 Connect the mouse with its USB cable or 2.4 GHz receiver, then rescan.\n",
            );
        } else {
            msg.push_str("\nDetected MCHOSE interfaces:\n");
            for node in &mchose {
                msg.push_str(&format!(
                    "  {}  {:04x}:{:04x}  {}\n      not a vendor config interface\n",
                    node.path.display(),
                    node.vid,
                    node.pid,
                    node.name
                ));
            }
        }
    } else {
        msg.push_str("\nDetected MCHOSE interfaces:\n");
        for (node, reason) in failures {
            msg.push_str(&format!(
                "  {}  {:04x}:{:04x}  {}\n      {}\n",
                node.path.display(),
                node.vid,
                node.pid,
                node.name,
                reason
            ));
        }
    }

    let permission = failures
        .iter()
        .any(|(_, e)| e.to_ascii_lowercase().contains("permission"));
    if permission {
        msg.push_str(
            "\nThe hidraw nodes are root-only by default. Install the udev rule once:\n  \
             sudo install -m644 70-mchose.rules /etc/udev/rules.d/70-mchose.rules\n  \
             sudo udevadm control --reload-rules && sudo udevadm trigger\n  \
             then unplug and replug the mouse / receiver.\n",
        );
    }

    let unsupported_family = !mchose.is_empty()
        && mchose.iter().all(|n| !n.is_config())
        && mchose
            .iter()
            .any(|n| n.vid == 0x2023 || n.name.to_ascii_uppercase().contains("G3"));
    if unsupported_family {
        msg.push_str(
            "\nThis looks like an A5/AX5 Pro or G3-family mouse. Those use a\n\
             different protocol and are not supported by this tool yet.\n",
        );
    }

    msg.push_str("\nPress r to rescan, q to quit.");
    msg
}

pub struct DemoMouse {
    cfg: Config,
    lod: u8,
    game: u8,
}

impl DemoMouse {
    pub fn new() -> Self {
        let mut cfg = Config { body: [0u8; 63] };
        cfg.body[0] = 0;
        cfg.body[1] = (2 << 4) | 2;
        cfg.body[2] = (2 << 4) | 2;
        let stages = [400u16, 800, 1600, 3200, 6400, 42000];
        for (i, value) in stages.iter().enumerate() {
            cfg.set_dpi(i, *value);
        }
        cfg.body[16] = 6;
        cfg.body[17] = 0x01 | 0x10;
        cfg.body[18] = 4;
        cfg.body[19] = 10;
        cfg.body[49] = 30;
        Self {
            cfg,
            lod: 1,
            game: 1,
        }
    }
}

impl Mouse for DemoMouse {
    fn name(&self) -> &str {
        "MCHOSE L7 Ultra (demo)"
    }

    fn path(&self) -> &str {
        "demo"
    }

    fn identity(&mut self) -> io::Result<Identity> {
        Ok(Identity {
            vid: 0x5253,
            pid: 0x00b1,
            firmware: 0x050c0201,
            connect_mode: 1,
            connected: true,
            battery: 87,
            charging: 1,
        })
    }

    fn version(&mut self) -> io::Result<String> {
        Ok("5.12.2.1".into())
    }

    fn read_config(&mut self) -> io::Result<Config> {
        Ok(self.cfg.clone())
    }

    fn write_config(&mut self, config: &Config) -> io::Result<()> {
        self.cfg = config.clone();
        Ok(())
    }

    fn confirm_config(&mut self, _config: &Config) -> io::Result<()> {
        Ok(())
    }

    fn set_rate(&mut self, index: u8) -> io::Result<()> {
        self.cfg.body[1] = (index << 4) | (self.cfg.body[1] & 0x0F);
        self.cfg.body[2] = (index << 4) | (self.cfg.body[2] & 0x0F);
        Ok(())
    }

    fn set_flags(
        &mut self,
        lod: u8,
        ripple: bool,
        angle_snap: bool,
        motion_sync: bool,
        _rotate: u8,
    ) -> io::Result<()> {
        self.lod = lod;
        let wanted = (self.cfg.sensor() & !0x1C)
            | ((ripple as u8) << 2)
            | ((angle_snap as u8) << 3)
            | ((motion_sync as u8) << 4);
        self.cfg.set_sensor(wanted);
        Ok(())
    }

    fn set_profile(&mut self, profile: u8) -> io::Result<()> {
        self.cfg.body[0] = profile;
        Ok(())
    }

    fn set_game_mode(&mut self, mode: u8) -> io::Result<()> {
        self.game = mode;
        Ok(())
    }

    fn game_mode(&mut self) -> io::Result<u8> {
        Ok(self.game)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l7_ultra_is_42000() {
        let m = model_info(0x00b1, "RealTek MCHOSE L7 Ultra");
        assert_eq!(m.dpi_max, 42000);
        assert_eq!(m.lod.len(), 3);
        assert_eq!(m.default_lod, 1);
        assert_eq!(m.rates.last().copied(), Some(8000));
    }

    #[test]
    fn l7_pro_is_26000() {
        let m = model_info(0x00b0, "MCHOSE L7 Pro");
        assert_eq!(m.dpi_max, 26000);
        assert_eq!(m.lod, vec![1.0, 2.0]);
    }
}

# mchose-tui

A tiny, dependency-free terminal UI for configuring **MCHOSE** gaming mice on Linux.

DPI stages, polling rate, lift-off distance, debounce, sleep, motion sync,
ripple control, angle snap, button remapping, profiles, game mode, battery and
charging — all from one keyboard-driven screen. No vendor driver, no browser,
no daemon.

```
 MCHOSE TUI 0.1.0  ·  MCHOSE L7 Ultra (demo)                               2.4 GHz  ·  87% charging
────────────────────────────────────────────────────────────────────────────────────────────────────
 DEVICE
   Battery                     ███████████░  87%  charging
   Link                        2.4 GHz  ·  online
   Firmware                    5.12.2.1
   Interface                   demo  (5253:00b1)
 PERFORMANCE
> Active DPI stage            3 / 6   (1600 DPI)
  DPI stage 1                 400 DPI
  DPI stage 2                 800 DPI
  DPI stage 3                 1600 DPI  · active
  DPI stage 4                 3200 DPI
  DPI stage 5                 6400 DPI
  DPI stage 6                 42000 DPI
  Polling rate                1000 Hz
  Debounce                    4 ms
  Sleep                       10 min
 SENSOR
  Lift-off distance           1.0 mm
  Motion sync                 on
  Ripple control              off
  Angle snap                  off
 BUTTONS
  Left button                 Default (native)
  Middle button               Default (native)
  Right button                Default (native)
  Forward button              Default (native)
  Back button                 Default (native)
 PROFILE
  Profile                     1 / 3
  Game mode                   1
 TOOLS
  Refresh                     re-read everything from the mouse
  Backup config               save a copy of the current config
────────────────────────────────────────────────────────────────────────────────────────────────────
 Ready. Changes are written immediately and verified.
 Up/Down move  Left/Right adjust  Enter edit/remap  r refresh  b backup  q quit
```

## Features

- **Battery and charging** in the header, refreshed every 15 seconds.
- **Six DPI stages** with an inline editor; the active stage is marked.
- **Polling rate** (125 / 500 / 1000 / 2000 / 4000 / 8000 Hz, model dependent).
- **Lift-off distance** (0.7 / 1.0 / 2.0 mm, model dependent).
- **Debounce** (0–30 ms) and **sleep timeout** (0–60 min, 0 = off).
- **Sensor flags**: motion sync, ripple control, angle snap.
- **Button remapping** for the five remappable buttons: native action, mouse
  buttons, DPI switches, media keys, system actions, profiles, disable, and 142
  keyboard keys and shortcuts. The picker filters as you type. The DPI button
  is fixed by the firmware — M HUB does not expose it either.
- **Profiles** 1–3 and **game mode** 1–3.
- **Backup / restore** of the 63-byte configuration block.
- **Frame log** (`f`) showing every HID frame sent and received.
- **Demo mode** (`--demo`) to explore the UI without hardware.

Every change is read back and compared with what was requested, so the screen
shows what the mouse actually holds. On the 42 000 DPI models (L7 Ultra and
friends) the lift-off index is mirrored in the config block's sensor byte and
is read back like everything else. On the 26 000 DPI models the firmware has no
read command for it, so the displayed value is the last one this tool wrote
(kept in `~/.local/state/mchose-tui/`).

## Supported mice

The tool speaks the MCHOSE **L7-family** protocol: vendor HID collection
`0xFF01` / `0xFF0B`, feature reports `0x11` (20 bytes) and `0x12` (64 bytes).
That covers, at least:

**L7** · **L7 Pro** · **L7 Ultra** · **L7 Ultra+** · **M7** · **A7** · **K7**

The model is identified from the HID name and product ID, which selects the DPI
ceiling, the LOD list and the polling-rate list. An L7 Ultra (`5253:00b1`) gets
42 000 DPI and 0.7/1.0/2.0 mm; other models default to 26 000 DPI and
1.0/2.0 mm. Unknown models still work with those safe defaults. The 42 000 DPI
models also mirror the LOD index in the sensor byte, so it is read back instead
of being remembered locally.

**Not supported:** the **A5 / AX5 Pro** family (`2023:...`, usage page `0xFFFF`,
65-byte frames) and the **G3** family (interrupt pipes, no feature reports).
They are detected and reported as unsupported instead of being written to.

## Build

Rust 1.87 or newer. No crates, no `hidapi`, no `libusb`, no C dependency beyond
the system libc.

```sh
cargo build --release
./target/release/mchose-tui --demo
```

The binary is `target/release/mchose-tui` and is self-contained.

## Install

```sh
./install.sh
```

This builds the binary, copies it to `~/.local/bin/mchose-tui`, and installs the
udev rule (one `sudo`). Then **replug the mouse or its receiver**.

Manual install:

```sh
install -Dm755 target/release/mchose-tui ~/.local/bin/mchose-tui
sudo install -Dm644 70-mchose.rules /etc/udev/rules.d/70-mchose.rules
sudo udevadm control --reload-rules
sudo udevadm trigger
```

The udev rule is required: the vendor configuration node is `root:root 0600`
by default, which is why the vendor web driver also fails silently.

## Usage

```sh
mchose-tui                  # autodetect the mouse
mchose-tui --demo           # simulated L7 Ultra, no hardware needed
mchose-tui --list           # show MCHOSE hidraw nodes and permissions
mchose-tui --device /dev/hidraw7
mchose-tui --no-color
```

### Keys

| Key | Action |
| --- | --- |
| `Up` / `Down`, `k` / `j` | move the selection |
| `Left` / `Right`, `h` / `l` | adjust the selected value |
| `Enter` / `Space` | apply, toggle, edit, remap, or run an action |
| `r` | re-read everything from the mouse |
| `b` | save a backup of the current config |
| `f` | show / hide the HID frame log |
| `q` / `Ctrl-C` | quit |

DPI values are edited inline: press `Enter` on a stage, type the new value, and
press `Enter` again. `Esc` cancels. Values must be between 50 and the model's
maximum, in steps of 50.

### Button remapping

Press `Enter` on any row in the **BUTTONS** group. The group lists the five
remappable buttons: **Left**, **Middle**, **Right**, **Forward**, and **Back**.
The **DPI button** is fixed by the firmware and is not offered for remapping,
just as in M HUB. Press `Enter` on a row and the screen becomes a picker:

| Key | Action |
| --- | --- |
| letters / digits / space | filter the action list (also matches the group) |
| `Backspace` | delete the last filter character |
| `Up` / `Down`, `PgUp` / `PgDn`, `Home` / `End` | move through the results |
| `Enter` | write the selected action to the mouse |
| `Esc` | cancel without writing |

Actions are grouped as **Default**, **Mouse**, **DPI**, **Media**, **System**,
**Keyboard**, **Profile**, and **Other** (which holds *Disable*). The mapping is
written with the dedicated `0x12 0x52` command and then read back from the
config block, so the row shows what the mouse actually stored.

The tool refuses to remove the **last left-click** button. If you want to move
left click elsewhere, first map another button to **Left click**, then change
the original one. The config backup made before the first write can always be
restored from **Tools → Restore original**.

## Safety

- Every write is a read-modify-write of the full 63-byte config block, then
  polled back until the mouse confirms it. Button remaps use the dedicated
  `0x12 0x52` command and are read back from the same config block.
- The remap picker refuses to leave the mouse without a left click.
- The first write of any kind saves the untouched config to
  `~/.local/state/mchose-tui/config.original.bin` (never overwritten).
- **Restore original** in the Tools group writes that file back; it asks for a
  second `Enter`.
- `b` writes the current config to `config.backup.bin`.
- No firmware is written, ever.

## Troubleshooting

**`permission denied` / the startup screen lists `/dev/hidrawN`**
Install the udev rule above and replug the device. Check with:

```sh
mchose-tui --list
```

**`receiver is idle (no mouse connected)`**
Move the mouse to wake it, turn it on, or connect the USB cable.

**The config channel stops answering**
The firmware can wedge its long-report channel while the short reports still
work. The tool re-selects the profile automatically; if that does not help,
unplug and replug the receiver.

**Polling rate / DPI is rejected**
The receiver or sensor does not support that value. The error line shows what
the mouse reported. 1K links reject 2K–8K rates.

**Lift-off distance looks wrong**
On the 26 000 DPI models it is write-only: the value shown is the last value
this tool wrote, not something read back from the mouse. The mouse may have had
a different value before the first run. On the 42 000 DPI models the value is
read back from the config block, so the screen shows the real index.

## Notes

- The tool talks to `/dev/hidraw*` directly, so it works the same on X11,
  Wayland, a TTY, or over SSH (with a terminal).
- Only the L7-family protocol is implemented. Pull requests for the A5/AX5 Pro
  and G3 families are welcome; the protocol is different enough to need a
  separate backend.
- The UI is deliberately small: one screen, no config file, no daemon, no
  telemetry.

## Credits

The HID protocol was reverse-engineered from MCHOSE's M HUB web driver and
documented in [icepie/mchose-linux](https://github.com/icepie/mchose-linux)
(also published as
[alexfrih/mchose-linux](https://github.com/alexfrih/mchose-linux), MIT).
This project is an independent TUI built on that documentation.

MCHOSE is a trademark of its owner. This project is unofficial and is not
affiliated with or endorsed by MCHOSE.

## License

MIT. See [LICENSE](LICENSE).

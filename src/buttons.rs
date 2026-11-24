//! Mouse button actions and the tables used to remap them.
//!
//! Every action is a `(kind, value)` pair. `kind` is the category byte the
//! firmware expects; `value` is the 24-bit action code stored big-endian in
//! the config block. The tables mirror M HUB's own action maps.

pub const KIND_NATIVE: u8 = 0;
pub const KIND_MOUSE: u8 = 1;
pub const KIND_KEYBOARD: u8 = 2;
pub const KIND_MEDIA: u8 = 3;
pub const KIND_MACRO: u8 = 4;
pub const KIND_DPI: u8 = 5;
pub const KIND_SYSTEM: u8 = 8;
pub const KIND_DISABLE: u8 = 9;
pub const KIND_PROFILE: u8 = 10;

pub const MOUSE_LEFT: u32 = 0x010000;
pub const DISABLE: u32 = 0x00ff_ffff;

#[derive(Clone, Copy)]
pub struct ButtonInfo {
    /// Position in the config block's button array (offset 20 + slot * 4).
    pub slot: u8,
    /// Position in the `0x12 0x52` command; the order differs.
    pub wire: u8,
    pub name: &'static str,
}

pub const BUTTONS: [ButtonInfo; 6] = [
    ButtonInfo {
        slot: 0,
        wire: 0,
        name: "Left",
    },
    ButtonInfo {
        slot: 1,
        wire: 2,
        name: "Right",
    },
    ButtonInfo {
        slot: 2,
        wire: 1,
        name: "Middle",
    },
    ButtonInfo {
        slot: 3,
        wire: 3,
        name: "Forward",
    },
    ButtonInfo {
        slot: 4,
        wire: 4,
        name: "Back",
    },
    ButtonInfo {
        slot: 5,
        wire: 5,
        name: "DPI",
    },
];

pub fn button_info(slot: u8) -> Option<&'static ButtonInfo> {
    BUTTONS.iter().find(|b| b.slot == slot)
}

pub fn slot_for_wire(wire: u8) -> Option<u8> {
    BUTTONS.iter().find(|b| b.wire == wire).map(|b| b.slot)
}

#[derive(Clone, Copy, Debug)]
pub struct Action {
    pub group: &'static str,
    pub name: &'static str,
    pub kind: u8,
    pub value: u32,
}

const NATIVE_ACTIONS: &[Action] = &[Action {
    group: "Default",
    name: "Default (native)",
    kind: KIND_NATIVE,
    value: 0,
}];

const MOUSE_ACTIONS: &[Action] = &[
    Action {
        group: "Mouse",
        name: "Left click",
        kind: KIND_MOUSE,
        value: MOUSE_LEFT,
    },
    Action {
        group: "Mouse",
        name: "Right click",
        kind: KIND_MOUSE,
        value: 0x020000,
    },
    Action {
        group: "Mouse",
        name: "Middle click",
        kind: KIND_MOUSE,
        value: 0x040000,
    },
    Action {
        group: "Mouse",
        name: "Forward",
        kind: KIND_MOUSE,
        value: 0x100000,
    },
    Action {
        group: "Mouse",
        name: "Back",
        kind: KIND_MOUSE,
        value: 0x080000,
    },
    Action {
        group: "Mouse",
        name: "Scroll up",
        kind: KIND_MOUSE,
        value: 0x000200,
    },
    Action {
        group: "Mouse",
        name: "Scroll down",
        kind: KIND_MOUSE,
        value: 0x00fe00,
    },
];

const DPI_ACTIONS: &[Action] = &[
    Action {
        group: "DPI",
        name: "Switch",
        kind: KIND_DPI,
        value: 0x010000,
    },
    Action {
        group: "DPI",
        name: "+",
        kind: KIND_DPI,
        value: 0x020000,
    },
    Action {
        group: "DPI",
        name: "-",
        kind: KIND_DPI,
        value: 0x030000,
    },
];

const MEDIA_ACTIONS: &[Action] = &[
    Action {
        group: "Media",
        name: "Volume +",
        kind: KIND_MEDIA,
        value: 0xe90000,
    },
    Action {
        group: "Media",
        name: "Volume -",
        kind: KIND_MEDIA,
        value: 0xea0000,
    },
    Action {
        group: "Media",
        name: "Mute",
        kind: KIND_MEDIA,
        value: 0xe20000,
    },
    Action {
        group: "Media",
        name: "Play/Pause",
        kind: KIND_MEDIA,
        value: 0xcd0000,
    },
    Action {
        group: "Media",
        name: "Previous",
        kind: KIND_MEDIA,
        value: 0xb60000,
    },
    Action {
        group: "Media",
        name: "Next",
        kind: KIND_MEDIA,
        value: 0xb50000,
    },
    Action {
        group: "Media",
        name: "Stop",
        kind: KIND_MEDIA,
        value: 0xb70000,
    },
];

const SYSTEM_ACTIONS: &[Action] = &[
    Action {
        group: "System",
        name: "Brightness +",
        kind: KIND_SYSTEM,
        value: 0x0c6f00,
    },
    Action {
        group: "System",
        name: "Brightness -",
        kind: KIND_SYSTEM,
        value: 0x0c7000,
    },
    Action {
        group: "System",
        name: "Copy",
        kind: KIND_SYSTEM,
        value: 0x070106,
    },
    Action {
        group: "System",
        name: "Cut",
        kind: KIND_SYSTEM,
        value: 0x07011b,
    },
    Action {
        group: "System",
        name: "Paste",
        kind: KIND_SYSTEM,
        value: 0x070119,
    },
];

const PROFILE_ACTIONS: &[Action] = &[
    Action {
        group: "Profile",
        name: "Profile 1",
        kind: KIND_PROFILE,
        value: 0x010000,
    },
    Action {
        group: "Profile",
        name: "Profile 2",
        kind: KIND_PROFILE,
        value: 0x020000,
    },
    Action {
        group: "Profile",
        name: "Profile 3",
        kind: KIND_PROFILE,
        value: 0x030000,
    },
    Action {
        group: "Profile",
        name: "Cycle profiles",
        kind: KIND_PROFILE,
        value: 0x040000,
    },
];

const OTHER_ACTIONS: &[Action] = &[Action {
    group: "Other",
    name: "Disable",
    kind: KIND_DISABLE,
    value: DISABLE,
}];

/// All remappable actions in display order.
pub fn actions() -> Vec<Action> {
    let mut all = Vec::with_capacity(
        NATIVE_ACTIONS.len()
            + MOUSE_ACTIONS.len()
            + DPI_ACTIONS.len()
            + MEDIA_ACTIONS.len()
            + SYSTEM_ACTIONS.len()
            + PROFILE_ACTIONS.len()
            + OTHER_ACTIONS.len()
            + KEYBOARD.len(),
    );
    all.extend_from_slice(NATIVE_ACTIONS);
    all.extend_from_slice(MOUSE_ACTIONS);
    all.extend_from_slice(DPI_ACTIONS);
    all.extend_from_slice(MEDIA_ACTIONS);
    all.extend_from_slice(SYSTEM_ACTIONS);
    for (name, value) in KEYBOARD {
        all.push(Action {
            group: "Keyboard",
            name,
            kind: KIND_KEYBOARD,
            value: *value,
        });
    }
    all.extend_from_slice(PROFILE_ACTIONS);
    all.extend_from_slice(OTHER_ACTIONS);
    all
}

/// Case-insensitive filter over the action name and its group.
pub fn filter(query: &str) -> Vec<Action> {
    let needle = query.trim().to_ascii_lowercase();
    actions()
        .into_iter()
        .filter(|action| {
            needle.is_empty()
                || action.name.to_ascii_lowercase().contains(&needle)
                || action.group.to_ascii_lowercase().contains(&needle)
        })
        .collect()
}

fn find_action(list: &'static [Action], value: u32) -> Option<&'static str> {
    list.iter()
        .find(|action| action.value == value)
        .map(|action| action.name)
}

fn find_keyboard(value: u32) -> Option<&'static str> {
    KEYBOARD
        .iter()
        .find(|(_, code)| *code == value)
        .map(|(name, _)| *name)
}

/// Human-readable name for a stored `(kind, value)` pair.
pub fn describe(kind: u8, value: u32) -> String {
    let name = match kind {
        KIND_NATIVE if value == 0 => Some("Default (native)"),
        KIND_MOUSE => find_action(MOUSE_ACTIONS, value),
        KIND_KEYBOARD => find_keyboard(value),
        KIND_MEDIA => find_action(MEDIA_ACTIONS, value),
        KIND_DPI => find_action(DPI_ACTIONS, value),
        KIND_SYSTEM => find_action(SYSTEM_ACTIONS, value),
        KIND_DISABLE => Some("Disabled"),
        KIND_PROFILE => find_action(PROFILE_ACTIONS, value),
        _ => None,
    };
    match name {
        Some(name) => name.to_string(),
        None if kind == KIND_MACRO => format!("macro 0x{value:06x}"),
        None => format!("type {kind}, value 0x{value:06x}"),
    }
}

const KEYBOARD: &[(&str, u32)] = &[
    ("A", 0x000400),
    ("B", 0x000500),
    ("C", 0x000600),
    ("D", 0x000700),
    ("E", 0x000800),
    ("F", 0x000900),
    ("G", 0x000a00),
    ("H", 0x000b00),
    ("I", 0x000c00),
    ("J", 0x000d00),
    ("K", 0x000e00),
    ("L", 0x000f00),
    ("M", 0x001000),
    ("N", 0x001100),
    ("O", 0x001200),
    ("P", 0x001300),
    ("Q", 0x001400),
    ("R", 0x001500),
    ("S", 0x001600),
    ("T", 0x001700),
    ("U", 0x001800),
    ("V", 0x001900),
    ("W", 0x001a00),
    ("X", 0x001b00),
    ("Y", 0x001c00),
    ("Z", 0x001d00),
    ("0 )", 0x002700),
    ("1 !", 0x001e00),
    ("2 @", 0x001f00),
    ("3 #", 0x002000),
    ("4 $", 0x002100),
    ("5 %", 0x002200),
    ("6 ^", 0x002300),
    ("7 &", 0x002400),
    ("8 *", 0x002500),
    ("9 (", 0x002600),
    ("- _", 0x002d00),
    (", <", 0x003600),
    ("; :", 0x003300),
    (". >", 0x003700),
    ("' \"", 0x003400),
    ("[ {", 0x002f00),
    ("] }", 0x003000),
    ("/ ?", 0x003800),
    ("\\ |", 0x003100),
    ("` ~", 0x003500),
    ("= +", 0x002e00),
    ("F1", 0x003a00),
    ("F2", 0x003b00),
    ("F3", 0x003c00),
    ("F4", 0x003d00),
    ("F5", 0x003e00),
    ("F6", 0x003f00),
    ("F7", 0x004000),
    ("F8", 0x004100),
    ("F9", 0x004200),
    ("F10", 0x004300),
    ("F11", 0x004400),
    ("F12", 0x004500),
    ("Delete", 0x004c00),
    ("Down", 0x005100),
    ("End", 0x004d00),
    ("Home", 0x004a00),
    ("Insert", 0x004900),
    ("Left", 0x005000),
    ("Page Down", 0x004e00),
    ("Page Up", 0x004b00),
    ("Right", 0x004f00),
    ("Up", 0x005200),
    ("Backspace", 0x002a00),
    ("Caps Lock", 0x003900),
    ("Enter", 0x002800),
    ("Esc", 0x002900),
    ("Menu", 0x006500),
    ("Num Lock", 0x005300),
    ("Print Screen", 0x004600),
    ("Pause", 0x004800),
    ("Scroll Lock", 0x004700),
    ("Space", 0x002c00),
    ("Tab", 0x002b00),
    ("Left Windows", 0x00e300),
    ("Right Windows", 0x00e700),
    ("Left Alt", 0x00e200),
    ("Right Alt", 0x00e600),
    ("Left Ctrl", 0x00e000),
    ("Right Ctrl", 0x00e400),
    ("Left Shift", 0x00e100),
    ("Right Shift", 0x00e500),
    ("Num -", 0x005600),
    ("Num *", 0x005500),
    ("Num /", 0x005400),
    ("Num +", 0x005700),
    ("Num 0", 0x006200),
    ("Num 1", 0x005900),
    ("Num 2", 0x005a00),
    ("Num 3", 0x005b00),
    ("Num 4", 0x005c00),
    ("Num 5", 0x005d00),
    ("Num 6", 0x005e00),
    ("Num 7", 0x005f00),
    ("Num 8", 0x006000),
    ("Num 9", 0x006100),
    ("Num Del", 0x006300),
    ("Num Enter", 0x005800),
    ("Ctrl + -", 0x012d00),
    ("Ctrl + Y", 0x011c00),
    ("Ctrl + =", 0x012e00),
    ("Ctrl + 0", 0x012700),
    ("Ctrl + C", 0x010600),
    ("Ctrl + A", 0x010400),
    ("Ctrl + N", 0x011100),
    ("Ctrl + X", 0x011b00),
    ("Ctrl + S", 0x011600),
    ("Ctrl + V", 0x011900),
    ("Ctrl + Z", 0x011d00),
    ("Ctrl + O", 0x011200),
    ("Ctrl + T", 0x011700),
    ("Ctrl + W", 0x011a00),
    ("Alt + Left", 0x045000),
    ("Alt + Right", 0x044f00),
    ("LWindows + tab", 0x082b00),
    ("LWindows + D", 0x080700),
    ("LWindows + Ctrl + Enter", 0x092800),
    ("LWindows + T", 0x081700),
    ("LWindows + A", 0x080400),
    ("LWindows + E", 0x080800),
    ("LWindows + I", 0x080c00),
    ("LWindows + L", 0x080f00),
    ("LWindows + K", 0x080e00),
    ("LWindows + B", 0x080500),
    ("LWindows + G", 0x080a00),
    ("LWindows + R", 0x081500),
    ("LWindows + S", 0x081600),
    ("LWindows + U", 0x081800),
    ("LWindows + X", 0x081b00),
    ("LWindows + .", 0x083700),
    ("LWindows + =", 0x082e00),
    ("Ctrl + Shift + Esc", 0x032900),
    ("Ctrl + Esc", 0x012900),
    ("Alt + Tab", 0x042b00),
    ("Alt + F4", 0x043d00),
    ("Alt + Esc", 0x042900),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_order_is_not_the_same_as_config_order() {
        assert_eq!(slot_for_wire(0), Some(0)); // left
        assert_eq!(slot_for_wire(1), Some(2)); // middle
        assert_eq!(slot_for_wire(2), Some(1)); // right
        assert_eq!(slot_for_wire(3), Some(3)); // forward
        assert_eq!(slot_for_wire(4), Some(4)); // back
        assert_eq!(slot_for_wire(5), Some(5)); // dpi
        assert_eq!(slot_for_wire(6), None);
    }

    #[test]
    fn every_action_fits_the_24_bit_field() {
        for action in actions() {
            assert!(action.value <= 0x00ff_ffff, "{action:?}");
            assert!(action.kind <= 0x0f, "{action:?}");
        }
    }

    #[test]
    fn describe_knows_defaults_and_tables() {
        assert_eq!(describe(KIND_NATIVE, 0), "Default (native)");
        assert_eq!(describe(KIND_MOUSE, MOUSE_LEFT), "Left click");
        assert_eq!(describe(KIND_KEYBOARD, 0x000400), "A");
        assert_eq!(describe(KIND_MEDIA, 0xe90000), "Volume +");
        assert_eq!(describe(KIND_DPI, 0x020000), "+");
        assert_eq!(describe(KIND_SYSTEM, 0x070106), "Copy");
        assert_eq!(describe(KIND_DISABLE, DISABLE), "Disabled");
        assert_eq!(describe(KIND_PROFILE, 0x040000), "Cycle profiles");
        assert_eq!(describe(KIND_MACRO, 0x123456), "macro 0x123456");
        assert_eq!(describe(6, 0x42), "type 6, value 0x000042");
    }

    #[test]
    fn filter_matches_names_and_groups() {
        let left = filter("left click");
        assert!(left
            .iter()
            .any(|a| a.kind == KIND_MOUSE && a.value == MOUSE_LEFT));
        let keys = filter("keyboard");
        assert!(!keys.is_empty());
        assert!(keys.iter().all(|a| a.kind == KIND_KEYBOARD));
        assert!(filter("").len() > keys.len());
        assert!(filter("no such action").is_empty());
    }
}

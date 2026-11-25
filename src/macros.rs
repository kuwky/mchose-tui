//! Text syntax for short mouse macros.
//!
//! A macro is a whitespace-separated list of steps:
//!
//! * `ctrl+c`, `shift+f10`, `alt+tab` — modifiers joined with `+`
//! * `a`, `f5`, `enter`, `pageup` — one key press
//! * `delay:50` — wait 50 ms
//! * `mouse:left`, `mouse:right`, `wheel:up` — a mouse action
//! * `"Hello"` — type the characters in the quotes
//!
//! Every key press becomes a key-down and key-up event, so a plain key costs
//! two of the thirteen events a short macro can hold. A chord costs four.

use crate::buttons;
use crate::proto::{MacroEvent, MACRO_MAX_EVENTS};

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Word(String),
    Text(String),
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(&ch) = chars.peek() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }
        if ch == '"' {
            chars.next();
            let mut text = String::new();
            let mut closed = false;
            for ch in chars.by_ref() {
                if ch == '"' {
                    closed = true;
                    break;
                }
                text.push(ch);
            }
            if !closed {
                return Err("Unclosed quote.".into());
            }
            tokens.push(Token::Text(text));
            continue;
        }
        let mut word = String::new();
        while let Some(&ch) = chars.peek() {
            if ch.is_whitespace() {
                break;
            }
            word.push(ch);
            chars.next();
        }
        tokens.push(Token::Word(word));
    }
    Ok(tokens)
}

fn push_char(events: &mut Vec<MacroEvent>, ch: char) -> Result<(), String> {
    let (code, shift) =
        buttons::char_key(ch).ok_or_else(|| format!("Cannot type {ch:?} on a US keyboard."))?;
    if shift {
        events.push(MacroEvent::Key {
            code: 0xe1,
            down: true,
        });
    }
    events.push(MacroEvent::Key { code, down: true });
    events.push(MacroEvent::Key { code, down: false });
    if shift {
        events.push(MacroEvent::Key {
            code: 0xe1,
            down: false,
        });
    }
    Ok(())
}

fn push_chord(events: &mut Vec<MacroEvent>, word: &str) -> Result<(), String> {
    let parts: Vec<&str> = word.split('+').collect();
    if parts.iter().any(|part| part.is_empty()) {
        return Err(format!("Bad chord {word:?}; use a key name for '+'."));
    }
    let (key_name, modifiers) = parts
        .split_last()
        .expect("split always returns at least one part");
    let mut codes = Vec::new();
    for modifier in modifiers {
        let code = buttons::modifier_code(modifier)
            .ok_or_else(|| format!("Unknown modifier {modifier:?} in {word:?}."))?;
        if !codes.contains(&code) {
            codes.push(code);
        }
    }
    let key = buttons::key_code(key_name)
        .ok_or_else(|| format!("Unknown key {key_name:?} in {word:?}."))?;

    for code in &codes {
        events.push(MacroEvent::Key {
            code: *code,
            down: true,
        });
    }
    events.push(MacroEvent::Key {
        code: key,
        down: true,
    });
    events.push(MacroEvent::Key {
        code: key,
        down: false,
    });
    for code in codes.iter().rev() {
        events.push(MacroEvent::Key {
            code: *code,
            down: false,
        });
    }
    Ok(())
}

fn push_word(events: &mut Vec<MacroEvent>, word: &str) -> Result<(), String> {
    let lower = word.to_ascii_lowercase();
    if let Some(rest) = lower
        .strip_prefix("delay:")
        .or_else(|| lower.strip_prefix("sleep:"))
        .or_else(|| lower.strip_prefix("wait:"))
    {
        let ms: u16 = rest
            .parse()
            .map_err(|_| format!("Delay {rest:?} must be 0-65535 milliseconds."))?;
        events.push(MacroEvent::Delay(ms));
        return Ok(());
    }
    if let Some(rest) = lower
        .strip_prefix("wheel:")
        .or_else(|| lower.strip_prefix("scroll:"))
    {
        match rest {
            "up" => events.push(MacroEvent::Wheel { up: true }),
            "down" => events.push(MacroEvent::Wheel { up: false }),
            _ => return Err(format!("Wheel direction {rest:?} must be up or down.")),
        }
        return Ok(());
    }
    if let Some(rest) = lower.strip_prefix("mouse:") {
        let button = match rest {
            "left" => 1,
            "right" => 2,
            "middle" | "mid" => 4,
            "forward" => 16,
            "back" => 8,
            _ => {
                return Err(format!(
                    "Mouse button {rest:?} must be left, right, middle, forward or back."
                ))
            }
        };
        events.push(MacroEvent::Mouse { button, down: true });
        events.push(MacroEvent::Mouse {
            button,
            down: false,
        });
        return Ok(());
    }
    push_chord(events, word)
}

/// Parse a macro expression into wire events.
pub fn parse(input: &str) -> Result<Vec<MacroEvent>, String> {
    let mut events = Vec::new();
    for token in tokenize(input)? {
        match token {
            Token::Text(text) => {
                for ch in text.chars() {
                    push_char(&mut events, ch)?;
                }
            }
            Token::Word(word) => push_word(&mut events, &word)?,
        }
    }
    if events.is_empty() {
        return Err("Macro is empty.".into());
    }
    if events.len() > MACRO_MAX_EVENTS {
        return Err(format!(
            "Macro has {} events; a short macro holds {} ({} bytes).",
            events.len(),
            MACRO_MAX_EVENTS,
            MACRO_MAX_EVENTS * 4
        ));
    }
    Ok(events)
}

/// A short description for the status line.
pub fn summary(events: &[MacroEvent]) -> String {
    format!(
        "{} event{} · {}/{} bytes",
        events.len(),
        if events.len() == 1 { "" } else { "s" },
        events.len() * 4,
        MACRO_MAX_EVENTS * 4
    )
}

/// Device macro names are limited to 48 bytes by the writer.
pub fn name_for(expression: &str) -> String {
    let mut name = String::from("TUI ");
    for ch in expression.trim().chars() {
        if name.len() + ch.len_utf8() > 48 {
            break;
        }
        name.push(ch);
    }
    if name == "TUI " {
        name.push_str("macro");
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: u8, down: bool) -> MacroEvent {
        MacroEvent::Key { code, down }
    }

    #[test]
    fn a_plain_key_is_down_and_up() {
        assert_eq!(parse("a").unwrap(), vec![key(0x04, true), key(0x04, false)]);
        assert_eq!(
            parse("ENTER").unwrap(),
            vec![key(0x28, true), key(0x28, false)]
        );
    }

    #[test]
    fn chords_press_and_release_modifiers_in_order() {
        assert_eq!(
            parse("ctrl+shift+c").unwrap(),
            vec![
                key(0xe0, true),
                key(0xe1, true),
                key(0x06, true),
                key(0x06, false),
                key(0xe1, false),
                key(0xe0, false),
            ]
        );
    }

    #[test]
    fn quoted_text_uses_shift_for_uppercase_and_symbols() {
        assert_eq!(
            parse("\"A!\"").unwrap(),
            vec![
                key(0xe1, true),
                key(0x04, true),
                key(0x04, false),
                key(0xe1, false),
                key(0xe1, true),
                key(0x1e, true),
                key(0x1e, false),
                key(0xe1, false),
            ]
        );
    }

    #[test]
    fn delays_and_mouse_steps_are_supported() {
        assert_eq!(parse("delay:50").unwrap(), vec![MacroEvent::Delay(50)]);
        assert_eq!(
            parse("mouse:left").unwrap(),
            vec![
                MacroEvent::Mouse {
                    button: 1,
                    down: true
                },
                MacroEvent::Mouse {
                    button: 1,
                    down: false
                },
            ]
        );
        assert_eq!(
            parse("wheel:down").unwrap(),
            vec![MacroEvent::Wheel { up: false }]
        );
    }

    #[test]
    fn bad_input_is_rejected_with_a_reason() {
        assert!(parse("").unwrap_err().contains("empty"));
        assert!(parse("nope").unwrap_err().contains("Unknown key"));
        assert!(parse("hyper+a").unwrap_err().contains("Unknown modifier"));
        assert!(parse("\"unterminated").unwrap_err().contains("quote"));
        assert!(parse("mouse:side").unwrap_err().contains("Mouse button"));
        assert!(parse("delay:99999").unwrap_err().contains("milliseconds"));

        let too_long = "a a a a a a a"; // 14 events
        assert!(parse(too_long).unwrap_err().contains("holds 13"));
    }

    #[test]
    fn names_are_short_and_never_empty() {
        assert_eq!(name_for("ctrl+c"), "TUI ctrl+c");
        assert_eq!(name_for("   "), "TUI macro");
        let long = name_for(&"a".repeat(100));
        assert!(long.len() <= 48);
    }
}

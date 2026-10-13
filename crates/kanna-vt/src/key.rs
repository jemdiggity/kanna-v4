use crate::{check, ffi, Error, Terminal};
use std::ffi::c_void;
use std::fmt;
use std::ptr::{self, NonNull};
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    name: String,
    code: i32,
    mods: u16,
    text: Option<char>,
    unshifted: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    Missing,
    AppOnly(String),
    UnknownModifier(String),
    UnknownKey(String),
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyError::Missing => write!(f, "a key is missing"),
            KeyError::AppOnly(key) => write!(f, "{key} only means something to the app, not to the program in the terminal"),
            KeyError::UnknownModifier(key) => write!(f, "{key}: modifiers are ctrl, alt and shift"),
            KeyError::UnknownKey(key) => write!(f, "\"{key}\" isn't a key"),
        }
    }
}

impl std::error::Error for KeyError {}

const PUNCTUATION: [(char, char, i32); 11] = [
    ('`', '~', ffi::GHOSTTY_KEY_BACKQUOTE),
    ('-', '_', ffi::GHOSTTY_KEY_MINUS),
    ('=', '+', ffi::GHOSTTY_KEY_EQUAL),
    ('[', '{', ffi::GHOSTTY_KEY_BRACKET_LEFT),
    (']', '}', ffi::GHOSTTY_KEY_BRACKET_RIGHT),
    ('\\', '|', ffi::GHOSTTY_KEY_BACKSLASH),
    (';', ':', ffi::GHOSTTY_KEY_SEMICOLON),
    ('\'', '"', ffi::GHOSTTY_KEY_QUOTE),
    (',', '<', ffi::GHOSTTY_KEY_COMMA),
    ('.', '>', ffi::GHOSTTY_KEY_PERIOD),
    ('/', '?', ffi::GHOSTTY_KEY_SLASH),
];

const SHIFTED_DIGITS: [char; 10] = [')', '!', '@', '#', '$', '%', '^', '&', '*', '('];

fn physical(c: char) -> Option<(char, char, i32)> {
    if c.is_ascii_lowercase() || c.is_ascii_uppercase() {
        let plain = c.to_ascii_lowercase();
        return Some((plain, plain.to_ascii_uppercase(), ffi::GHOSTTY_KEY_A + (plain as i32 - 'a' as i32)));
    }
    if let Some(digit) = c.to_digit(10) {
        return Some((c, SHIFTED_DIGITS[digit as usize], ffi::GHOSTTY_KEY_DIGIT_0 + digit as i32));
    }
    if let Some(at) = SHIFTED_DIGITS.iter().position(|s| *s == c) {
        return Some((char::from_digit(at as u32, 10)?, c, ffi::GHOSTTY_KEY_DIGIT_0 + at as i32));
    }
    PUNCTUATION.iter().copied().find(|(plain, shifted, _)| *plain == c || *shifted == c)
}

fn named(name: &str) -> Option<(i32, Option<char>)> {
    let code = match name {
        "return" | "enter" => ffi::GHOSTTY_KEY_ENTER,
        "esc" | "escape" => ffi::GHOSTTY_KEY_ESCAPE,
        "tab" => ffi::GHOSTTY_KEY_TAB,
        "space" => return Some((ffi::GHOSTTY_KEY_SPACE, Some(' '))),
        "backspace" | "delete" => ffi::GHOSTTY_KEY_BACKSPACE,
        "forward-delete" | "forwarddelete" => ffi::GHOSTTY_KEY_DELETE,
        "up" => ffi::GHOSTTY_KEY_ARROW_UP,
        "down" => ffi::GHOSTTY_KEY_ARROW_DOWN,
        "left" => ffi::GHOSTTY_KEY_ARROW_LEFT,
        "right" => ffi::GHOSTTY_KEY_ARROW_RIGHT,
        "home" => ffi::GHOSTTY_KEY_HOME,
        "end" => ffi::GHOSTTY_KEY_END,
        "pageup" | "page-up" | "pgup" => ffi::GHOSTTY_KEY_PAGE_UP,
        "pagedown" | "page-down" | "pgdn" => ffi::GHOSTTY_KEY_PAGE_DOWN,
        "insert" => ffi::GHOSTTY_KEY_INSERT,
        _ => {
            let n: i32 = name.strip_prefix('f')?.parse().ok().filter(|n| (1..=12).contains(n))?;
            ffi::GHOSTTY_KEY_F1 + n - 1
        }
    };
    Some((code, None))
}

impl FromStr for Key {
    type Err = KeyError;

    fn from_str(spec: &str) -> Result<Key, KeyError> {
        let (prefix, last) = if spec == "+" {
            ("", "+")
        } else if let Some(prefix) = spec.strip_suffix("++") {
            (prefix, "+")
        } else {
            spec.rsplit_once('+').unwrap_or(("", spec))
        };
        let mut mods = 0;
        for modifier in prefix.split('+').filter(|_| !prefix.is_empty()) {
            mods |= match modifier.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => ffi::GHOSTTY_MODS_CTRL,
                "alt" | "opt" | "option" => ffi::GHOSTTY_MODS_ALT,
                "shift" => ffi::GHOSTTY_MODS_SHIFT,
                "cmd" | "command" | "super" => return Err(KeyError::AppOnly(spec.to_string())),
                _ => return Err(KeyError::UnknownModifier(spec.to_string())),
            };
        }
        let mut chars = last.chars();
        let (code, text, unshifted) = match (chars.next(), chars.next()) {
            (None, _) => return Err(KeyError::Missing),
            (Some(c), None) if c.is_control() => return Err(KeyError::UnknownKey(spec.to_string())),
            (Some(' '), None) => (ffi::GHOSTTY_KEY_SPACE, Some(' '), ' ' as u32),
            (Some(c), None) => match physical(c) {
                Some((plain, shifted, code)) => {
                    if c != plain {
                        mods |= ffi::GHOSTTY_MODS_SHIFT;
                    }
                    let shift = mods & ffi::GHOSTTY_MODS_SHIFT != 0;
                    (code, Some(if shift { shifted } else { plain }), plain as u32)
                }
                None => (ffi::GHOSTTY_KEY_UNIDENTIFIED, Some(c), c as u32),
            },
            _ => match named(&last.to_ascii_lowercase()) {
                Some((code, text)) => (code, text, text.map_or(0, |c| c as u32)),
                None => return Err(KeyError::UnknownKey(spec.to_string())),
            },
        };
        Ok(Key { name: spec.to_string(), code, mods, text, unshifted })
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

struct Encoder(NonNull<ffi::GhosttyKeyEncoderImpl>);

impl Drop for Encoder {
    fn drop(&mut self) {
        unsafe { ffi::ghostty_key_encoder_free(self.0.as_ptr()) };
    }
}

struct Event(NonNull<ffi::GhosttyKeyEventImpl>);

impl Drop for Event {
    fn drop(&mut self) {
        unsafe { ffi::ghostty_key_event_free(self.0.as_ptr()) };
    }
}

impl Terminal {
    pub fn encode_key(&self, key: &Key) -> Result<Vec<u8>, Error> {
        let mut raw: ffi::GhosttyKeyEncoder = ptr::null_mut();
        check("ghostty_key_encoder_new", unsafe { ffi::ghostty_key_encoder_new(ptr::null(), &mut raw) })?;
        let encoder = Encoder(NonNull::new(raw).ok_or(Error { op: "ghostty_key_encoder_new", code: ffi::GHOSTTY_INVALID_VALUE })?);
        let mut raw: ffi::GhosttyKeyEvent = ptr::null_mut();
        check("ghostty_key_event_new", unsafe { ffi::ghostty_key_event_new(ptr::null(), &mut raw) })?;
        let event = Event(NonNull::new(raw).ok_or(Error { op: "ghostty_key_event_new", code: ffi::GHOSTTY_INVALID_VALUE })?);
        let mut utf8 = [0u8; 4];
        let text = key.text.map(|c| c.encode_utf8(&mut utf8).len());
        let alt = ffi::GHOSTTY_OPTION_AS_ALT_TRUE;
        let consumed = if text.is_some() { key.mods & ffi::GHOSTTY_MODS_SHIFT } else { 0 };
        let mut out = [0u8; 128];
        let mut len = 0usize;
        unsafe {
            ffi::ghostty_key_encoder_setopt_from_terminal(encoder.0.as_ptr(), self.raw.as_ptr());
            ffi::ghostty_key_encoder_setopt(encoder.0.as_ptr(), ffi::GHOSTTY_KEY_ENCODER_OPT_MACOS_OPTION_AS_ALT, &alt as *const i32 as *const c_void);
            ffi::ghostty_key_event_set_action(event.0.as_ptr(), ffi::GHOSTTY_KEY_ACTION_PRESS);
            ffi::ghostty_key_event_set_key(event.0.as_ptr(), key.code);
            ffi::ghostty_key_event_set_mods(event.0.as_ptr(), key.mods);
            ffi::ghostty_key_event_set_consumed_mods(event.0.as_ptr(), consumed);
            match text {
                Some(n) => ffi::ghostty_key_event_set_utf8(event.0.as_ptr(), utf8.as_ptr(), n),
                None => ffi::ghostty_key_event_set_utf8(event.0.as_ptr(), ptr::null(), 0),
            }
            ffi::ghostty_key_event_set_unshifted_codepoint(event.0.as_ptr(), key.unshifted);
            check(
                "ghostty_key_encoder_encode",
                ffi::ghostty_key_encoder_encode(encoder.0.as_ptr(), event.0.as_ptr(), out.as_mut_ptr(), out.len(), &mut len),
            )?;
        }
        Ok(out[..len].to_vec())
    }
}

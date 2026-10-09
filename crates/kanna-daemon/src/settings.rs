use kanna_mux::json::{json, num, Value};
use kanna_vt::{Colors, CursorStyle, Mode, Rgb, Scheme, Terminal};

pub const MAX_SCROLLBACK_BYTES: usize = 10_000_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub colors: Option<Colors>,
    pub scheme: Option<Scheme>,
    pub cursor_style: CursorStyle,
    pub cursor_blink: bool,
    pub graphemes: bool,
    pub scrollback_bytes: usize,
    pub scrollback_lines: Option<usize>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            colors: None,
            scheme: None,
            cursor_style: CursorStyle::Block,
            cursor_blink: true,
            graphemes: true,
            scrollback_bytes: MAX_SCROLLBACK_BYTES,
            scrollback_lines: None,
        }
    }
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn parse_hex(value: &Value) -> Option<Rgb> {
    let text = value.as_str()?.strip_prefix('#')?;
    if text.len() != 6 || !text.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&text[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

fn style_name(style: CursorStyle) -> &'static str {
    match style {
        CursorStyle::Bar => "bar",
        CursorStyle::Block => "block",
        CursorStyle::Underline => "underline",
        CursorStyle::BlockHollow => "block_hollow",
    }
}

impl Settings {
    pub fn from_value(value: &Value) -> Settings {
        let defaults = Settings::default();
        let colors = (|| {
            let palette: Vec<Rgb> = value.get("palette")?.as_array()?.iter().map(parse_hex).collect::<Option<_>>()?;
            Some(Colors {
                foreground: parse_hex(value.get("foreground")?)?,
                background: parse_hex(value.get("background")?)?,
                cursor: value.get("cursor").and_then(parse_hex),
                palette: Box::new(palette.try_into().ok()?),
            })
        })();
        let limit = |key: &str| value.get(key).and_then(Value::as_f64).filter(|n| *n >= 0.0).map(|n| n.min(usize::MAX as f64) as usize);
        Settings {
            colors,
            scheme: match value.get("scheme").and_then(Value::as_str) {
                Some("light") => Some(Scheme::Light),
                Some("dark") => Some(Scheme::Dark),
                _ => None,
            },
            cursor_style: match value.get("cursorStyle").and_then(Value::as_str) {
                Some("bar") => CursorStyle::Bar,
                Some("underline") => CursorStyle::Underline,
                Some("block_hollow") => CursorStyle::BlockHollow,
                _ => CursorStyle::Block,
            },
            cursor_blink: value.get("cursorBlink").and_then(Value::as_bool).unwrap_or(defaults.cursor_blink),
            graphemes: value.get("graphemes").and_then(Value::as_bool).unwrap_or(defaults.graphemes),
            scrollback_bytes: limit("scrollbackBytes").map_or(MAX_SCROLLBACK_BYTES, |n| n.min(MAX_SCROLLBACK_BYTES)),
            scrollback_lines: limit("scrollbackLines"),
        }
    }

    pub fn to_value(&self) -> Value {
        let mut value = json!({
            "cursorStyle": style_name(self.cursor_style),
            "cursorBlink": self.cursor_blink,
            "graphemes": self.graphemes,
            "scrollbackBytes": num(self.scrollback_bytes as f64),
        });
        if let Some(lines) = self.scrollback_lines {
            value["scrollbackLines"] = num(lines as f64);
        }
        if let Some(scheme) = self.scheme {
            value["scheme"] = Value::String(if scheme == Scheme::Dark { "dark" } else { "light" }.into());
        }
        if let Some(colors) = &self.colors {
            value["foreground"] = Value::String(hex(colors.foreground));
            value["background"] = Value::String(hex(colors.background));
            if let Some(cursor) = colors.cursor {
                value["cursor"] = Value::String(hex(cursor));
            }
            value["palette"] = Value::Array(colors.palette.iter().map(|c| Value::String(hex(*c))).collect());
        }
        value
    }

    pub fn start(&self, screen: &mut Terminal) {
        let _ = screen.set_mode_default(Mode::GRAPHEME_CLUSTER, self.graphemes);
        let _ = screen.set_mode_default(Mode::CURSOR_BLINKING, self.cursor_blink);
        self.apply(screen);
    }

    pub fn apply(&self, screen: &mut Terminal) {
        if let Some(colors) = &self.colors {
            let _ = screen.set_default_colors(colors);
        }
        screen.set_color_scheme(self.scheme);
        let _ = screen.set_default_cursor(self.cursor_style, self.cursor_blink);
        let _ = screen.set_scrollback_bytes(Some(self.scrollback_bytes));
        let _ = screen.set_scrollback_line_limit(self.scrollback_lines);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_are_bounded() {
        let mut palette = Box::new([[0u8; 3]; 256]);
        palette[7] = [1, 2, 3];
        let settings = Settings {
            colors: Some(Colors { foreground: [0xe5, 0xd4, 0xb0], background: [0x12, 0x34, 0x56], cursor: None, palette }),
            scheme: Some(Scheme::Dark),
            cursor_style: CursorStyle::Bar,
            cursor_blink: false,
            graphemes: false,
            scrollback_bytes: 1000,
            scrollback_lines: Some(50),
        };
        assert_eq!(Settings::from_value(&settings.to_value()), settings);
        let huge = Settings::from_value(&json!({ "scrollbackBytes": num(1e12), "palette": ["#zzzzzz"] }));
        assert_eq!(huge.scrollback_bytes, MAX_SCROLLBACK_BYTES);
        assert_eq!(huge.colors, None);
        assert_eq!(Settings::from_value(&json!({})), Settings::default());
    }
}

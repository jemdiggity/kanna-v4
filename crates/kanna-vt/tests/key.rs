use kanna_vt::{Key, KeyError, Terminal};

fn bytes(terminal: &Terminal, key: &str) -> Vec<u8> {
    terminal.encode_key(&key.parse::<Key>().unwrap()).unwrap()
}

fn sent(modes: &str, keys: &[&str]) -> Vec<String> {
    let mut terminal = Terminal::new(80, 24, 0).unwrap();
    terminal.write(modes.as_bytes());
    keys.iter().map(|key| String::from_utf8_lossy(&bytes(&terminal, key)).into_owned()).collect()
}

#[test]
fn named_keys_and_letters() {
    assert_eq!(
        sent("", &["return", "Enter", "esc", "tab", "shift+tab", "space", "backspace", "delete", "forward-delete", "a", "A", "shift+a", "?", "1", "shift+1", "+", "é"]),
        ["\r", "\r", "\x1b", "\t", "\x1b[Z", " ", "\x7f", "\x7f", "\x1b[3~", "a", "A", "A", "?", "1", "!", "+", "é"]
    );
    assert_eq!(sent("", &["home", "end", "pageup", "page-down", "insert", "f1", "f5", "F12"]), ["\x1b[H", "\x1b[F", "\x1b[5~", "\x1b[6~", "\x1b[2~", "\x1bOP", "\x1b[15~", "\x1b[24~"]);
}

#[test]
fn ctrl_and_alt() {
    assert_eq!(sent("", &["ctrl+c", "ctrl+C", "control+d", "ctrl+[", "ctrl+space", "alt+x", "opt+b", "ctrl+alt+a"]), ["\x03", "\x1b[99;6u", "\x04", "\x1b[91;5u", "\x00", "\x1bx", "\x1bb", "\x1b\x01"]);
}

#[test]
fn arrows_follow_the_cursor_key_mode() {
    assert_eq!(sent("", &["up", "down", "right", "left"]), ["\x1b[A", "\x1b[B", "\x1b[C", "\x1b[D"]);
    assert_eq!(sent("\x1b[?1h", &["up", "down", "right", "left", "home"]), ["\x1bOA", "\x1bOB", "\x1bOC", "\x1bOD", "\x1bOH"]);
    assert_eq!(sent("\x1b[?1h\x1b[?1l", &["up"]), ["\x1b[A"]);
    assert_eq!(sent("\x1b[?1h", &["shift+up", "ctrl+left"]), ["\x1b[1;2A", "\x1b[1;5D"]);
}

#[test]
fn the_kitty_protocol_when_the_program_asks_for_it() {
    assert_eq!(sent("\x1b[>1u", &["esc", "ctrl+c", "a", "return", "shift+tab", "up"]), ["\x1b[27u", "\x1b[99;5u", "a", "\r", "\x1b[9;2u", "\x1b[A"]);
    assert_eq!(sent("\x1b[>1u\x1b[<u", &["esc", "ctrl+c"]), ["\x1b", "\x03"]);
}

#[test]
fn modify_other_keys_tells_ctrl_digits_apart() {
    assert_eq!(sent("", &["ctrl+1", "ctrl+2"]), ["1", "\x00"]);
    assert_eq!(sent("\x1b[>4;2m", &["ctrl+1", "ctrl+c"]), ["\x1b[27;5;49~", "\x1b[27;5;99~"]);
}

#[test]
fn what_isnt_a_key() {
    assert_eq!("cmd+c".parse::<Key>(), Err(KeyError::AppOnly("cmd+c".into())));
    assert_eq!("super+shift+d".parse::<Key>(), Err(KeyError::AppOnly("super+shift+d".into())));
    assert_eq!("hyper+a".parse::<Key>(), Err(KeyError::UnknownModifier("hyper+a".into())));
    assert_eq!("hello".parse::<Key>(), Err(KeyError::UnknownKey("hello".into())));
    assert_eq!("f13".parse::<Key>(), Err(KeyError::UnknownKey("f13".into())));
    assert_eq!("ctrl+".parse::<Key>(), Err(KeyError::Missing));
    assert_eq!("\n".parse::<Key>(), Err(KeyError::UnknownKey("\n".into())));
    assert_eq!("cmd+c".parse::<Key>().unwrap_err().to_string(), "cmd+c only means something to the app, not to the program in the terminal");
    assert_eq!("ctrl+c".parse::<Key>().unwrap().to_string(), "ctrl+c");
}

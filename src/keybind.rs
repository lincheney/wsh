use std::io::{Write, Cursor};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, KeyEventState};

fn write_app_keypad<W: Write>(key: KeyEvent, mut cursor: W) -> bool {
    if !key.state.contains(KeyEventState::KEYPAD) {
        return false
    }

    let numlock = key.state.contains(KeyEventState::NUM_LOCK);
    let string: &[u8] = match key.code {
        KeyCode::Tab                        => b"\x1bOI",
        KeyCode::Char(' ')                  => b"\x1bO ",
        KeyCode::Enter          if !numlock => b"\x1bOM",
        KeyCode::Char('*')      if !numlock => b"\x1bOj",
        KeyCode::Char('+')      if !numlock => b"\x1bOk",
        KeyCode::Char('.')      if !numlock => b"\x1bOl",
        KeyCode::Char('-')      if !numlock => b"\x1bOm",
        KeyCode::Char(',')      if !numlock => b"\x1bOn",
        KeyCode::Delete         if !numlock => b"\x1bO3~",
        KeyCode::Char('/')      if !numlock => b"\x1bOo",
        _ => return false,
    };
    cursor.write_all(string).unwrap();
    true
}

fn write_csi_keys<W: Write>(key: KeyEvent, mut cursor: W) -> bool {
    let mut flags = 0;
    if key.modifiers.contains(KeyModifiers::SHIFT)   { flags |= 1; }
    if key.modifiers.contains(KeyModifiers::ALT)     { flags |= 2; }
    if key.modifiers.contains(KeyModifiers::CONTROL) { flags |= 4; }

    let (num, val)          = match key.code {
        KeyCode::Up        => (1, 'A'),
        KeyCode::Down      => (1, 'B'),
        KeyCode::Right     => (1, 'C'),
        KeyCode::Left      => (1, 'D'),
        KeyCode::End       => (1, 'F'),
        KeyCode::Home      => (1, 'H'),
        KeyCode::Insert    => (2, '~'),
        KeyCode::Delete    => (3, '~'),
        KeyCode::PageUp    => (5, '~'),
        KeyCode::PageDown  => (6, '~'),
        KeyCode::F(1)      => (0, 'P'),
        KeyCode::F(2)      => (0, 'Q'),
        KeyCode::F(3)      => (0, 'R'),
        KeyCode::F(4)      => (0, 'S'),
        KeyCode::F(5)      => (15, '~'),
        KeyCode::F(x @ 6..= 10)        => (x + 11, '~'),
        KeyCode::F(x @ 11..= 14)       => (x + 12, '~'),
        KeyCode::F(x @ 15..= 16)       => (x + 13, '~'),
        KeyCode::F(x @ 17..= 20)       => (x + 14, '~'),
        KeyCode::F(x @ 21..= 35)       => (x + 21, '~'),
        _ => return false,
    };

    if flags != 0 {
        let flags = flags + 1;
        write!(cursor, "\x1b[{num};{flags}{val}").unwrap();
    } else if num > 1 {
        write!(cursor, "\x1b[{num}{val}").unwrap();
    } else if num == 0 {
        write!(cursor, "\x1bO{val}").unwrap();
    } else {
        write!(cursor, "\x1b[{val}").unwrap();
    }
    true
}

fn keyevent_to_cursor<W: Write>(key: KeyEvent, mut cursor: W) {
    if write_app_keypad(key, &mut cursor) {
        return
    }
    if write_csi_keys(key, &mut cursor) {
        return
    }
    if key.code == KeyCode::BackTab || (key.code == KeyCode::Tab && key.modifiers.contains(KeyModifiers::SHIFT)) {
        cursor.write_all(b"\x1b[Z").unwrap();
        return
    }
    let val = if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char(c @ ('@'..='~' | ' ')) => Some(c as u8 & 0x1f),
            KeyCode::Char('2')                   => Some(b'\0'),
            KeyCode::Char(c @ '3'..='7')         => Some(c as u8 - b'3' + b'\x1b'),
            KeyCode::Char('8' | '?')             => Some(b'\x7f'),
            KeyCode::Char('-' | '/')             => Some(b'\x1f'),
            _ => None,
        }
    } else if key.code == KeyCode::Backspace {
        Some(b'\x7f')
    } else if key.code == KeyCode::Enter {
        Some(b'\r')
    } else {
        None
    };

    if key.modifiers.contains(KeyModifiers::ALT) {
        cursor.write_all(b"\x1b").unwrap();
    }
    if let Some(c) = val {
        cursor.write_all(&[c]).unwrap();
    } else if let KeyCode::Char(c) = key.code {
        write!(cursor, "{c}").unwrap();
    }

    return
}

pub fn keyevent_to_bytes<const N: usize>(key: KeyEvent, buf: &mut [u8; N]) -> Option<&[u8]> {
    let mut cursor = Cursor::new(buf as &mut [u8]);
    keyevent_to_cursor(key, &mut cursor);
    let len = cursor.position();
    if len == 0 {
        None
    } else {
        Some(&buf[..len as _])
    }
}

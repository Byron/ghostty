//! Input encoders consume terminal modes and leave platform event handling to hosts.
use crate::Terminal;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub super_key: bool,
    pub caps_lock: bool,
    pub num_lock: bool,
}

impl Modifiers {
    fn number(self) -> u16 {
        1 + u16::from(self.shift)
            + 2 * u16::from(self.alt)
            + 4 * u16::from(self.control)
            + 8 * u16::from(self.super_key)
    }
    fn kitty_number(self) -> u16 {
        self.number() + 64 * u16::from(self.caps_lock) + 128 * u16::from(self.num_lock)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    Function(u8),
    Keypad(u8),
    KeypadEnter,
    KeypadDecimal,
    KeypadAdd,
    KeypadSubtract,
    KeypadMultiply,
    KeypadDivide,
    Shift,
    Control,
    Alt,
    Super,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyAction {
    #[default]
    Press,
    Repeat,
    Release,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub text: Option<String>,
    pub modifiers: Modifiers,
    pub action: KeyAction,
    /// Logical unshifted Unicode key. `key` retains the physical/base-layout key.
    pub unshifted: Option<char>,
    pub composing: bool,
}

impl KeyEvent {
    pub fn new(key: Key) -> Self {
        Self {
            key,
            text: match key {
                Key::Char(cp) => Some(cp.to_string()),
                _ => None,
            },
            modifiers: Modifiers::default(),
            action: KeyAction::Press,
            unshifted: None,
            composing: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    Move,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    WheelLeft,
    WheelRight,
    Extra(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseEvent {
    pub action: MouseAction,
    pub button: Option<MouseButton>,
    /// Zero-based cell coordinates.
    pub col: usize,
    pub row: usize,
    /// Zero-based pixel coordinates for SGR-pixel mode.
    pub x: u32,
    pub y: u32,
    pub modifiers: Modifiers,
}

impl Terminal {
    pub fn encode_key(&self, event: &KeyEvent) -> Vec<u8> {
        if self.modes.get(false, 2) {
            return Vec::new();
        }
        let flags = self.screen().kitty_keyboard.last().copied().unwrap_or(0);
        if flags != 0 {
            return kitty_key(event, flags);
        }
        if event.action == KeyAction::Release || event.composing {
            return Vec::new();
        }
        let mods = event.modifiers;
        let number = mods.number();
        let text = event.text.as_deref().unwrap_or("");
        if matches!(event.key, Key::Enter | Key::Escape | Key::Backspace)
            && !text.is_empty()
            && text.chars().all(|cp| !cp.is_control())
        {
            return if event.key == Key::Backspace {
                Vec::new()
            } else {
                text.as_bytes().to_vec()
            };
        }
        if self.modify_other_keys
            && number > 1
            && let Some(cp) = text.chars().next().or(match event.key {
                Key::Char(cp) => Some(cp),
                Key::Enter => Some('\r'),
                Key::Tab => Some('\t'),
                Key::Backspace => Some('\u{7f}'),
                Key::Escape => Some('\u{1b}'),
                _ => None,
            })
        {
            return format!("\x1b[27;{number};{}~", cp as u32).into_bytes();
        }
        if let Some(final_byte) = match event.key {
            Key::Up => Some('A'),
            Key::Down => Some('B'),
            Key::Right => Some('C'),
            Key::Left => Some('D'),
            Key::Home => Some('H'),
            Key::End => Some('F'),
            _ => None,
        } {
            return if number > 1 {
                format!("\x1b[1;{number}{final_byte}")
            } else {
                format!(
                    "\x1b{}{final_byte}",
                    if self.modes.dec(1) { 'O' } else { '[' }
                )
            }
            .into_bytes();
        }
        if let Some((code, final_byte)) = functional(event.key) {
            if matches!(event.key, Key::Function(1..=4)) && number == 1 {
                let Key::Function(f) = event.key else {
                    unreachable!()
                };
                return vec![0x1b, b'O', b'P' + f - 1];
            }
            return if number == 1 {
                format!("\x1b[{code}{final_byte}")
            } else {
                format!("\x1b[{code};{number}{final_byte}")
            }
            .into_bytes();
        }
        if let Some((app, plain)) = keypad(event.key) {
            return if self.modes.dec(66) && !(self.modes.dec(1035) && mods.num_lock) {
                if number > 1 {
                    format!("\x1bO{number}{app}").into_bytes()
                } else {
                    vec![0x1b, b'O', app as u8]
                }
            } else {
                vec![plain as u8]
            };
        }
        if event.key == Key::Tab && mods.shift && !mods.control && !mods.alt {
            return b"\x1b[Z".to_vec();
        }
        let mut output = match event.key {
            Key::Enter => {
                if self.modes.get(false, 20) {
                    b"\r\n".to_vec()
                } else {
                    vec![b'\r']
                }
            }
            Key::Tab => vec![b'\t'],
            Key::Escape => vec![0x1b],
            Key::Backspace => vec![if self.modes.dec(67) || mods.control {
                0x08
            } else {
                0x7f
            }],
            Key::Char(cp) => {
                let base = event.unshifted.unwrap_or(cp);
                if mods.control {
                    if let Some(byte) = control_byte(base) {
                        vec![byte]
                    } else {
                        text.as_bytes().to_vec()
                    }
                } else if !text.is_empty() {
                    text.as_bytes().to_vec()
                } else {
                    cp.to_string().into_bytes()
                }
            }
            _ => Vec::new(),
        };
        if mods.alt && self.modes.dec(1036) && !output.is_empty() {
            output.insert(0, 0x1b);
        }
        output
    }

    pub fn encode_focus(&self, focused: bool) -> Vec<u8> {
        if self.modes.dec(1004) {
            if focused {
                b"\x1b[I".to_vec()
            } else {
                b"\x1b[O".to_vec()
            }
        } else {
            Vec::new()
        }
    }

    /// Reports whether the app should request confirmation before a clipboard paste.
    pub fn paste_is_safe(&self, text: &str) -> bool {
        !text.contains("\x1b[201~") && (self.modes.dec(2004) || !text.contains('\n'))
    }

    /// Low-level text insertion; callers enforce their clipboard confirmation policy.
    pub fn encode_paste(&self, text: &str) -> Vec<u8> {
        let bracketed = self.modes.dec(2004);
        let mut output = Vec::with_capacity(text.len() + 12);
        if bracketed {
            output.extend_from_slice(b"\x1b[200~");
        }
        for byte in text.bytes() {
            output.push(match byte {
                0x00 | 0x08 | 0x05 | 0x04 | 0x1b | 0x7f | 0x03 | 0x1c | 0x15 | 0x1a | 0x11
                | 0x13 | 0x17 | 0x16 | 0x12 | 0x0f => b' ',
                b'\n' if !bracketed => b'\r',
                _ => byte,
            });
        }
        if bracketed {
            output.extend_from_slice(b"\x1b[201~");
        }
        output
    }

    pub fn encode_mouse(&self, event: MouseEvent) -> Vec<u8> {
        let mode = self.mouse_mode;
        if mode == 0
            || mode == 9 && event.action != MouseAction::Press
            || event.action == MouseAction::Move
                && (mode == 1000 || mode == 1002 && event.button.is_none())
        {
            return Vec::new();
        }
        let mut code: u16 = match event.button {
            Some(MouseButton::Left) => 0,
            Some(MouseButton::Middle) => 1,
            Some(MouseButton::Right) => 2,
            Some(MouseButton::WheelUp) => 64,
            Some(MouseButton::WheelDown) => 65,
            Some(MouseButton::WheelLeft) => 66,
            Some(MouseButton::WheelRight) => 67,
            Some(MouseButton::Extra(n)) => 128 + u16::from(n.min(127)),
            None => 3,
        };
        let release = event.action == MouseAction::Release;
        if release && !matches!(self.mouse_format, 1006 | 1016) {
            code = 3;
        }
        if event.action == MouseAction::Move {
            code += 32;
        }
        if mode != 9 {
            code += u16::from(event.modifiers.shift) * 4
                + u16::from(event.modifiers.alt) * 8
                + u16::from(event.modifiers.control) * 16;
        }
        let (x, y) = if self.mouse_format == 1016 {
            (event.x as usize + 1, event.y as usize + 1)
        } else {
            (event.col.saturating_add(1), event.row.saturating_add(1))
        };
        match self.mouse_format {
            1006 | 1016 => {
                format!("\x1b[<{code};{x};{y}{}", if release { 'm' } else { 'M' }).into_bytes()
            }
            1015 => format!("\x1b[{};{x};{y}M", code + 32).into_bytes(),
            1005 if x <= 2015 && y <= 2015 => {
                let mut output = b"\x1b[M".to_vec();
                for n in [code as u32 + 32, x as u32 + 32, y as u32 + 32] {
                    output.extend_from_slice(char::from_u32(n).unwrap().to_string().as_bytes());
                }
                output
            }
            0 if x <= 223 && y <= 223 && code + 32 <= 255 => vec![
                0x1b,
                b'[',
                b'M',
                (code + 32) as u8,
                (x + 32) as u8,
                (y + 32) as u8,
            ],
            _ => Vec::new(),
        }
    }
}

fn control_byte(cp: char) -> Option<u8> {
    match cp {
        'a'..='z' => Some(cp as u8 - b'a' + 1),
        '@'..='_' => Some(cp as u8 - b'@'),
        ' ' | '2' => Some(0),
        '3' => Some(0x1b),
        '4' => Some(0x1c),
        '5' => Some(0x1d),
        '6' => Some(0x1e),
        '7' | '/' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
}

fn functional(key: Key) -> Option<(u32, char)> {
    Some(match key {
        Key::Insert => (2, '~'),
        Key::Delete => (3, '~'),
        Key::PageUp => (5, '~'),
        Key::PageDown => (6, '~'),
        Key::Function(1) => (1, 'P'),
        Key::Function(2) => (1, 'Q'),
        Key::Function(3) => (13, '~'),
        Key::Function(4) => (1, 'S'),
        Key::Function(f @ 5..=25) => (
            [
                15, 17, 18, 19, 20, 21, 23, 24, 25, 26, 28, 29, 31, 32, 33, 34, 42, 43, 44, 45, 46,
            ][(f - 5) as usize],
            '~',
        ),
        _ => return None,
    })
}

fn keypad(key: Key) -> Option<(char, char)> {
    Some(match key {
        Key::Keypad(n @ 0..=9) => ((b'p' + n) as char, (b'0' + n) as char),
        Key::KeypadEnter => ('M', '\r'),
        Key::KeypadDecimal => ('n', '.'),
        Key::KeypadAdd => ('k', '+'),
        Key::KeypadSubtract => ('m', '-'),
        Key::KeypadMultiply => ('j', '*'),
        Key::KeypadDivide => ('o', '/'),
        _ => return None,
    })
}

fn kitty_key(event: &KeyEvent, flags: u8) -> Vec<u8> {
    let all = flags & 8 != 0;
    let report_events = flags & 2 != 0;
    let modifier_key = matches!(event.key, Key::Shift | Key::Control | Key::Alt | Key::Super);
    if event.action == KeyAction::Release
        && (!report_events || !all && matches!(event.key, Key::Enter | Key::Tab | Key::Backspace))
        || event.composing && !modifier_key
        || modifier_key && !all
    {
        return Vec::new();
    }
    let text = event.text.as_deref().unwrap_or("");
    if !all && !event.modifiers.control && !event.modifiers.alt && !event.modifiers.super_key {
        if event.action != KeyAction::Release
            && !text.is_empty()
            && text.chars().all(|cp| !cp.is_control())
        {
            return text.as_bytes().to_vec();
        }
        if !event.modifiers.shift {
            match event.key {
                Key::Enter => return vec![b'\r'],
                Key::Tab => return vec![b'\t'],
                Key::Backspace => return vec![0x7f],
                _ => {}
            }
        }
    }
    let (code, final_byte) = match event.key {
        Key::Char(cp) => (event.unshifted.unwrap_or(cp) as u32, 'u'),
        Key::Enter => (13, 'u'),
        Key::Tab => (9, 'u'),
        Key::Backspace => (127, 'u'),
        Key::Escape => (27, 'u'),
        Key::Up => (1, 'A'),
        Key::Down => (1, 'B'),
        Key::Right => (1, 'C'),
        Key::Left => (1, 'D'),
        Key::Home => (1, 'H'),
        Key::End => (1, 'F'),
        Key::Keypad(n @ 0..=9) => (57399 + u32::from(n), 'u'),
        Key::KeypadDecimal => (57409, 'u'),
        Key::KeypadDivide => (57410, 'u'),
        Key::KeypadMultiply => (57411, 'u'),
        Key::KeypadSubtract => (57412, 'u'),
        Key::KeypadAdd => (57413, 'u'),
        Key::KeypadEnter => (57414, 'u'),
        Key::Shift => (57441, 'u'),
        Key::Control => (57442, 'u'),
        Key::Alt => (57443, 'u'),
        Key::Super => (57444, 'u'),
        _ => match functional(event.key) {
            Some(code) => code,
            None => return Vec::new(),
        },
    };
    let mut output = format!("\x1b[{code}");
    if flags & 4 != 0 && code > 31 && final_byte == 'u' {
        let shifted = text
            .chars()
            .next()
            .filter(|&cp| event.modifiers.shift && cp as u32 != code);
        let base = match event.key {
            Key::Char(cp) if cp as u32 != code => Some(cp),
            _ => None,
        };
        if let Some(cp) = shifted {
            output.push_str(&format!(":{}", cp as u32));
        }
        if let Some(cp) = base {
            if shifted.is_none() {
                output.push(':');
            }
            output.push_str(&format!(":{}", cp as u32));
        }
    }
    let mods = event.modifiers.kitty_number();
    let event_number = match event.action {
        KeyAction::Press => 1,
        KeyAction::Repeat => 2,
        KeyAction::Release => 3,
    };
    let associated = flags & 16 != 0
        && event.action != KeyAction::Release
        && !event.modifiers.control
        && !event.modifiers.alt
        && !event.modifiers.super_key
        && !text.is_empty();
    if mods != 1 || report_events && event_number != 1 || associated {
        output.push_str(&format!(";{mods}"));
        if report_events && event_number != 1 {
            output.push_str(&format!(":{event_number}"));
        }
    }
    if associated {
        output.push(';');
        for (index, cp) in text.chars().enumerate() {
            if index != 0 {
                output.push(':');
            }
            output.push_str(&(cp as u32).to_string());
        }
    }
    output.push(final_byte);
    output.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn key_modes_and_ime() {
        let mut t = Terminal::new(80, 24, 10);
        let mut key = KeyEvent::new(Key::Up);
        assert_eq!(t.encode_key(&key), b"\x1b[A");
        t.feed(b"\x1b[?1h");
        assert_eq!(t.encode_key(&key), b"\x1bOA");
        key.modifiers.control = true;
        assert_eq!(t.encode_key(&key), b"\x1b[1;5A");
        let mut key = KeyEvent::new(Key::Char('c'));
        key.modifiers.control = true;
        assert_eq!(t.encode_key(&key), [3]);
        t.feed(b"\x1b[>4;2m");
        assert_eq!(t.encode_key(&key), b"\x1b[27;5;99~");
        key.composing = true;
        assert!(t.encode_key(&key).is_empty());
        let mut key = KeyEvent::new(Key::Enter);
        key.text = Some("日本".into());
        assert_eq!(t.encode_key(&key), "日本".as_bytes());
    }
    #[test]
    fn kitty_reports_modifiers_repeats_release_and_associated_text() {
        let mut t = Terminal::new(80, 24, 10);
        t.feed(b"\x1b[>31u");
        let mut key = KeyEvent::new(Key::Char('a'));
        key.modifiers.control = true;
        assert_eq!(t.encode_key(&key), b"\x1b[97;5u");
        key.action = KeyAction::Repeat;
        assert_eq!(t.encode_key(&key), b"\x1b[97;5:2u");
        key.action = KeyAction::Release;
        assert_eq!(t.encode_key(&key), b"\x1b[97;5:3u");
        key = KeyEvent::new(Key::Char('a'));
        assert_eq!(t.encode_key(&key), b"\x1b[97;1;97u");
        t.feed(b"\x1b[<u");
        assert_eq!(t.encode_key(&key), b"a");
    }
    #[test]
    fn paste_frames_strip_unsafe_bytes_and_preserve_utf8() {
        let mut t = Terminal::new(80, 24, 10);
        assert!(!t.paste_is_safe("echo hi\n"));
        assert_eq!(t.encode_paste("é\x1b[201~\n"), "é [201~\r".as_bytes());
        t.feed(b"\x1b[?2004h");
        assert!(t.paste_is_safe("echo hi\n"));
        assert!(!t.paste_is_safe("\x1b[201~"));
        assert_eq!(t.encode_paste("a\nb"), b"\x1b[200~a\nb\x1b[201~");
    }
    #[test]
    fn mouse_protocol_and_focus() {
        let mut t = Terminal::new(80, 24, 10);
        let mut e = MouseEvent {
            action: MouseAction::Press,
            button: Some(MouseButton::Left),
            col: 3,
            row: 4,
            x: 30,
            y: 40,
            modifiers: Modifiers::default(),
        };
        assert!(t.encode_mouse(e).is_empty());
        t.feed(b"\x1b[?1000h\x1b[?1006h\x1b[?1004h");
        assert_eq!(t.encode_mouse(e), b"\x1b[<0;4;5M");
        e.action = MouseAction::Release;
        assert_eq!(t.encode_mouse(e), b"\x1b[<0;4;5m");
        e.action = MouseAction::Move;
        assert!(t.encode_mouse(e).is_empty());
        t.feed(b"\x1b[?1003h\x1b[?1016h");
        assert_eq!(t.encode_mouse(e), b"\x1b[<32;31;41M");
        assert_eq!(t.encode_focus(false), b"\x1b[O");
    }
}

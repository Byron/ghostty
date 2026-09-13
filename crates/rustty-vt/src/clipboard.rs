//! Clipboard requests leave policy and OS access with the host.

use base64::Engine;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location {
    Standard,
    Selection,
    Primary,
}

impl Location {
    pub(crate) fn from_selector(selector: u8) -> Self {
        match selector {
            b's' => Self::Selection,
            b'p' => Self::Primary,
            _ => Self::Standard,
        }
    }

    fn selector(self) -> u8 {
        match self {
            Self::Standard => b'c',
            Self::Selection => b's',
            Self::Primary => b'p',
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terminator {
    Bell,
    St,
}

impl Terminator {
    pub fn as_bytes(self) -> &'static [u8] {
        match self {
            Self::Bell => b"\x07",
            Self::St => b"\x1b\\",
        }
    }
}

/// A binary-safe clipboard representation. An empty representation list clears
/// the clipboard; a representation with empty data retains its MIME type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Content {
    pub mime: Vec<u8>,
    pub data: Vec<u8>,
}

pub fn is_text_mime(mime: &[u8]) -> bool {
    matches!(
        mime,
        b"text/plain" | b"text/plain;charset=utf-8" | b"UTF8_STRING" | b"TEXT" | b"STRING"
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Write {
    pub location: Location,
    pub contents: Vec<Content>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Read {
    pub location: Location,
    pub terminator: Terminator,
}

impl Read {
    /// Encode an OSC 52 reply using the first text representation. Pass an
    /// empty list when the clipboard is unavailable, access is denied, or no
    /// reply was supplied. Consuming the request keeps one reply per request.
    pub fn reply(self, contents: &[Content]) -> Vec<u8> {
        let mut reply = b"\x1b]52;".to_vec();
        reply.push(self.location.selector());
        reply.push(b';');
        if let Some(content) = contents.iter().find(|c| is_text_mime(&c.mime)) {
            reply.extend_from_slice(
                base64::engine::general_purpose::STANDARD
                    .encode(&content.data)
                    .as_bytes(),
            );
        }
        reply.extend_from_slice(self.terminator.as_bytes());
        reply
    }
}

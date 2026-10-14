//! Splitting the client's byte stream into the stream header, top-level
//! stanzas and the stream's end. An XMPP stream is one XML document that never
//! closes until the session ends, so a document parser can't be used directly:
//! this tracks tag depth, skipping quoted attribute values, comments, CDATA and
//! processing instructions (the client's `<?xml?>` declaration).

/// The largest stanza accepted, as the roadmap sets it.
pub const MAX_STANZA: usize = 64 * 1024;

#[derive(Debug, PartialEq)]
pub enum Frame {
    /// `<stream:stream …>`, the whole opening tag.
    StreamOpen(String),
    /// One top-level element, as sent.
    Stanza(String),
    /// `</stream:stream>`.
    StreamClose,
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum FrameError {
    #[error("stanza larger than {MAX_STANZA} bytes")]
    TooLarge,
    #[error("text between stanzas")]
    TextOutsideStanza,
    #[error("stanza is not UTF-8")]
    NotUtf8,
    #[error("unbalanced end tag")]
    Unbalanced,
}

#[derive(Default)]
pub struct Framer {
    buf: Vec<u8>,
}

impl Framer {
    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// The next complete frame, or `None` until more bytes arrive.
    pub fn next_frame(&mut self) -> Result<Option<Frame>, FrameError> {
        loop {
            let start = self.buf.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(self.buf.len());
            self.buf.drain(..start);
            if self.buf.is_empty() {
                return Ok(None);
            }
            if self.buf[0] != b'<' {
                return Err(FrameError::TextOutsideStanza);
            }
            let rest = &self.buf[..];
            if rest.starts_with(b"<?") {
                let Some(end) = find(rest, b"?>") else { return self.incomplete() };
                self.buf.drain(..end + 2);
                continue;
            }
            if rest.starts_with(b"</stream:stream") {
                let Some(end) = rest.iter().position(|&b| b == b'>') else { return self.incomplete() };
                self.buf.drain(..=end);
                return Ok(Some(Frame::StreamClose));
            }
            if rest.starts_with(b"<stream:stream") {
                let Some(end) = tag_end(rest, 0) else { return self.incomplete() };
                let tag = self.take(end)?;
                return Ok(Some(Frame::StreamOpen(tag)));
            }
            let Some(end) = element_end(rest)? else { return self.incomplete() };
            let stanza = self.take(end)?;
            return Ok(Some(Frame::Stanza(stanza)));
        }
    }

    fn incomplete(&self) -> Result<Option<Frame>, FrameError> {
        if self.buf.len() > MAX_STANZA { Err(FrameError::TooLarge) } else { Ok(None) }
    }

    /// Remove and return the first `end` bytes.
    fn take(&mut self, end: usize) -> Result<String, FrameError> {
        if end > MAX_STANZA {
            return Err(FrameError::TooLarge);
        }
        let bytes: Vec<u8> = self.buf.drain(..end).collect();
        String::from_utf8(bytes).map_err(|_| FrameError::NotUtf8)
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// The index just past the `>` that ends the tag starting at `at`, skipping
/// quoted attribute values.
fn tag_end(buf: &[u8], at: usize) -> Option<usize> {
    let mut quote = None;
    for (i, &b) in buf.iter().enumerate().skip(at + 1) {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => quote = Some(b),
            None if b == b'>' => return Some(i + 1),
            None => {}
        }
    }
    None
}

/// The index just past the element starting at 0, once it is complete.
fn element_end(buf: &[u8]) -> Result<Option<usize>, FrameError> {
    let mut depth = 0usize;
    let mut i = 0;
    while i < buf.len() {
        if buf[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &buf[i..];
        let skip =
            |open: &[u8], close: &[u8]| rest.starts_with(open).then(|| find(rest, close).map(|e| e + close.len()));
        if let Some(end) = skip(b"<!--", b"-->").or_else(|| skip(b"<![CDATA[", b"]]>")).or_else(|| skip(b"<?", b"?>")) {
            let Some(end) = end else { return Ok(None) };
            i += end;
            continue;
        }
        let Some(end) = tag_end(buf, i) else { return Ok(None) };
        if rest.starts_with(b"</") {
            depth = depth.checked_sub(1).ok_or(FrameError::Unbalanced)?;
        } else if buf[end - 2] != b'/' {
            depth += 1;
        }
        if depth == 0 {
            return Ok(Some(end));
        }
        i = end;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(input: &[&[u8]]) -> Vec<Frame> {
        let mut framer = Framer::default();
        let mut out = Vec::new();
        for chunk in input {
            framer.push(chunk);
            while let Some(frame) = framer.next_frame().unwrap() {
                out.push(frame);
            }
        }
        out
    }

    #[test]
    fn splits_the_stream_into_its_header_stanzas_and_end() {
        let got = frames(&[
            b"<?xml version='1.0' ?><stream:stream to='127.0.0.1' xmlns='jabber:client' version='1.0'>",
            b"<starttls xmlns='urn:ietf:params:xml:ns:xmpp-tls'/>  <iq id='a' type='get'><query xmlns='jabber:iq:auth'>",
            b"<username>sbrw.100</username></query></iq></stream:stream>",
        ]);
        assert_eq!(
            got,
            [
                Frame::StreamOpen("<stream:stream to='127.0.0.1' xmlns='jabber:client' version='1.0'>".into()),
                Frame::Stanza("<starttls xmlns='urn:ietf:params:xml:ns:xmpp-tls'/>".into()),
                Frame::Stanza(
                    "<iq id='a' type='get'><query xmlns='jabber:iq:auth'><username>sbrw.100</username></query></iq>"
                        .into()
                ),
                Frame::StreamClose,
            ]
        );
    }

    #[test]
    fn waits_for_split_tags_and_ignores_markup_in_quotes_comments_and_cdata() {
        let stanza =
            "<message to='a>b' x=\"</message>\"><!-- </message> --><body><![CDATA[</message>]]></body></message>";
        let bytes = stanza.as_bytes();
        let got = frames(&bytes.chunks(3).collect::<Vec<_>>());
        assert_eq!(got, [Frame::Stanza(stanza.into())]);
    }

    #[test]
    fn rejects_text_between_stanzas_and_oversized_stanzas() {
        let mut framer = Framer::default();
        framer.push(b"hello");
        assert_eq!(framer.next_frame(), Err(FrameError::TextOutsideStanza));

        let mut framer = Framer::default();
        framer.push(b"<message><body>");
        framer.push(&vec![b'a'; MAX_STANZA]);
        assert_eq!(framer.next_frame(), Err(FrameError::TooLarge));
    }
}

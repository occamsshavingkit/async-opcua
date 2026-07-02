use std::{
    io::{BufReader, Read},
    num::{ParseFloatError, ParseIntError},
    str::FromStr,
};

use quick_xml::{
    escape::{resolve_predefined_entity, EscapeError},
    events::{BytesRef, Event},
};
use thiserror::Error;

#[derive(Debug, Error)]
/// Error produced when reading XML.
pub enum XmlReadError {
    #[error("{0}")]
    /// Failed to parse XML.
    Xml(#[from] quick_xml::Error),
    #[error("Unexpected EOF")]
    /// Unexpected EOF.
    UnexpectedEof,
    #[error("Failed to parse integer: {0}")]
    /// Failed to parse value as integer.
    ParseInt(#[from] ParseIntError),
    #[error("Failed to parse float: {0}")]
    /// Failed to parse value as float.
    ParseFloat(#[from] ParseFloatError),
    #[error("Failed to parse value: {0}")]
    /// Some other parse error.
    Parse(String),
}

/// XML stream reader specialized for working with OPC-UA XML.
pub struct XmlStreamReader<T> {
    reader: quick_xml::Reader<BufReader<T>>,
    buffer: Vec<u8>,
}

fn decode_reference(reference: BytesRef<'_>) -> Result<String, quick_xml::Error> {
    if let Some(codepoint) = reference.resolve_char_ref()? {
        return Ok(codepoint.to_string());
    }

    let len = reference.len();
    let reference = reference.decode().map_err(quick_xml::Error::from)?;
    if let Some(value) = resolve_predefined_entity(reference.as_ref()) {
        return Ok(value.to_owned());
    }

    Err(EscapeError::UnrecognizedEntity(0..len, reference.into_owned()).into())
}

impl<T: Read> XmlStreamReader<T> {
    /// Create a new stream reader with an internal buffer.
    pub fn new(reader: T) -> Self {
        Self {
            reader: quick_xml::Reader::from_reader(BufReader::new(reader)),
            buffer: Vec::new(),
        }
    }

    /// Get the next event from the stream.
    pub fn next_event(&mut self) -> Result<quick_xml::events::Event<'_>, XmlReadError> {
        self.buffer.clear();
        Ok(self.reader.read_event_into(&mut self.buffer)?)
    }

    /// Skip the current value. This should be called after encountering a
    /// `Start` event, and will skip until the corresponding `End` event is consumed.
    ///
    /// Note that this does not check that the document is coherent, just that
    /// an equal number of start and end events are consumed.
    pub fn skip_value(&mut self) -> Result<(), XmlReadError> {
        let mut depth = 1u32;
        loop {
            match self.next_event()? {
                Event::Start(_) => depth += 1,
                Event::End(_) => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                Event::Eof => {
                    if depth == 1 {
                        return Ok(());
                    } else {
                        return Err(XmlReadError::UnexpectedEof);
                    }
                }
                _ => {}
            }
        }
    }

    /// Consume the current event, skipping any child elements and returning the combined text
    /// content with leading and trailing whitespace removed.
    /// Note that if there are multiple text elements they will be concatenated, but
    /// whitespace between these will not be removed.
    pub fn consume_as_text(&mut self) -> Result<String, XmlReadError> {
        let mut text: Option<String> = None;
        let mut depth = 1u32;
        loop {
            match self.next_event()? {
                Event::Start(_) => depth += 1,
                Event::End(_) => {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(mut text) = text {
                            let trimmed = text.trim_ascii_end();
                            text.truncate(trimmed.len());
                            return Ok(text);
                        } else {
                            return Ok(String::new());
                        }
                    }
                }
                Event::Text(mut e) => {
                    if depth != 1 {
                        continue;
                    }
                    if let Some(text) = text.as_mut() {
                        text.push_str(&e.decode().map_err(quick_xml::Error::from)?);
                    } else if e.inplace_trim_start() {
                        continue;
                    } else {
                        text = Some(e.decode().map_err(quick_xml::Error::from)?.into_owned());
                    }
                }
                Event::GeneralRef(e) => {
                    if depth != 1 {
                        continue;
                    }
                    let resolved = decode_reference(e)?;
                    if let Some(text) = text.as_mut() {
                        text.push_str(&resolved);
                    } else {
                        text = Some(resolved);
                    }
                }

                Event::Eof => {
                    if depth == 1 {
                        if let Some(mut text) = text {
                            let trimmed = text.trim_ascii_end();
                            text.truncate(trimmed.len());
                            return Ok(text);
                        } else {
                            return Ok(String::new());
                        }
                    } else {
                        return Err(XmlReadError::UnexpectedEof);
                    }
                }
                _ => continue,
            }
        }
    }

    /// Consume the current element as a raw array of bytes.
    pub fn consume_raw(&mut self) -> Result<Vec<u8>, XmlReadError> {
        let mut out = Vec::new();
        let mut depth = 1u32;
        // quick-xml doesn't really have a way to do this, and in fact does not capture the full event,
        // fortunately the way it does capture each event is quite predictable, so we can reconstruct
        // the input.
        // We do need the parser, since we only want to read the current element.
        loop {
            let evt = self.next_event()?;
            match evt {
                Event::Start(s) => {
                    depth += 1;
                    out.push(b'<');
                    out.extend_from_slice(&s);
                    out.push(b'>');
                }
                Event::End(s) => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(out);
                    }
                    out.extend_from_slice(b"</");
                    out.extend_from_slice(&s);
                    out.push(b'>');
                }
                Event::CData(s) => {
                    out.extend_from_slice(b"<![CDATA[");
                    out.extend_from_slice(&s);
                    out.extend_from_slice(b"]]>");
                }
                Event::Comment(s) => {
                    out.extend_from_slice(b"<!--");
                    out.extend_from_slice(&s);
                    out.extend_from_slice(b"-->");
                }
                Event::Decl(s) => {
                    out.extend_from_slice(b"<?");
                    out.extend_from_slice(&s);
                    out.extend_from_slice(b"?>");
                }
                Event::DocType(s) => {
                    out.extend_from_slice(b"<!DOCTYPE");
                    out.extend_from_slice(&s);
                    out.push(b'>');
                }
                Event::Empty(s) => {
                    out.push(b'<');
                    out.extend_from_slice(&s);
                    out.extend_from_slice(b"/>");
                }
                Event::PI(s) => {
                    out.extend_from_slice(b"<?");
                    out.extend_from_slice(&s);
                    out.extend_from_slice(b"?>");
                }
                Event::Text(s) => {
                    out.extend_from_slice(&s);
                }
                Event::GeneralRef(s) => {
                    out.push(b'&');
                    out.extend_from_slice(&s);
                    out.push(b';');
                }
                Event::Eof => {
                    if depth == 1 {
                        return Ok(out);
                    } else {
                        return Err(XmlReadError::UnexpectedEof);
                    }
                }
            }
        }
    }

    /// Consume the current node as a text value and parse it as the given type.
    pub fn consume_content<R: FromStr>(&mut self) -> Result<R, XmlReadError>
    where
        XmlReadError: From<<R as FromStr>::Err>,
    {
        let text = self.consume_as_text()?;
        Ok(text.parse()?)
    }
}

#[cfg(test)]
mod test {
    use std::io::Cursor;

    use quick_xml::events::Event;

    #[test]
    fn test_xml_text_comments() {
        let xml = r#"
        <Foo>
            Ho
            <Bar>
            Hello
            </Bar>
            Hello <!-- Comment --> there
        </Foo>
        "#;
        let mut cursor = Cursor::new(xml.as_bytes());
        let mut reader = super::XmlStreamReader::new(&mut cursor);
        // You canend up with text everywhere. Any loading needs to account for this.
        assert!(matches!(reader.next_event().unwrap(), Event::Text(_)));
        assert!(matches!(reader.next_event().unwrap(), Event::Start(_)));
        assert!(matches!(reader.next_event().unwrap(), Event::Text(_)));
        assert!(matches!(reader.next_event().unwrap(), Event::Start(_)));
        assert!(matches!(reader.next_event().unwrap(), Event::Text(_)));
        assert!(matches!(reader.next_event().unwrap(), Event::End(_)));
        assert!(matches!(reader.next_event().unwrap(), Event::Text(_)));
        assert!(matches!(reader.next_event().unwrap(), Event::Comment(_)));
        assert!(matches!(reader.next_event().unwrap(), Event::Text(_)));
        assert!(matches!(reader.next_event().unwrap(), Event::End(_)));
        assert!(matches!(reader.next_event().unwrap(), Event::Text(_)));
        assert!(matches!(reader.next_event().unwrap(), Event::Eof));
        assert!(matches!(reader.next_event().unwrap(), Event::Eof));
    }

    #[test]
    fn test_consume_as_text() {
        let xml = r#"<Foo>
            <Bar>
            Hello
            </Bar>
            Hello <!-- Comment -->there &lt; 6 &#x21;
        </Foo>"#;

        let mut cursor = Cursor::new(xml.as_bytes());
        let mut reader = super::XmlStreamReader::new(&mut cursor);

        assert!(matches!(reader.next_event().unwrap(), Event::Start(_)));
        assert_eq!(reader.consume_as_text().unwrap(), "Hello there < 6 !");
    }

    #[test]
    fn test_consume_content() {
        let xml = r#"<Foo>
            12345
        </Foo>"#;
        let mut cursor = Cursor::new(xml.as_bytes());
        let mut reader = super::XmlStreamReader::new(&mut cursor);

        assert!(matches!(reader.next_event().unwrap(), Event::Start(_)));
        assert_eq!(reader.consume_content::<u32>().unwrap(), 12345);
    }

    #[test]
    fn test_consume_raw() {
        let xml = r#"<Foo>
<Bar>
    Hello <!-- Comment here -->
    More text
</Bar>
<Bar attr = "foo" />
<? Mystery PI ?>
</Foo>"#;
        let mut cursor = Cursor::new(xml.as_bytes());
        let mut reader = super::XmlStreamReader::new(&mut cursor);
        assert!(matches!(reader.next_event().unwrap(), Event::Start(_)));
        let raw = reader.consume_raw().unwrap();
        println!("{}", String::from_utf8_lossy(&raw));
        assert_eq!(&xml.as_bytes()[5..(xml.len() - 6)], &*raw);
    }

    #[test]
    fn test_consume_raw_preserves_general_refs() {
        let xml = r#"<Foo>1 &lt; 2 &#x21;</Foo>"#;
        let mut cursor = Cursor::new(xml.as_bytes());
        let mut reader = super::XmlStreamReader::new(&mut cursor);

        assert!(matches!(reader.next_event().unwrap(), Event::Start(_)));
        let raw = reader.consume_raw().unwrap();
        assert_eq!(&b"1 &lt; 2 &#x21;"[..], &*raw);
    }
}

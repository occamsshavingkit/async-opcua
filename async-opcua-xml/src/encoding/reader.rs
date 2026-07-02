use std::{
    io::{BufReader, Read},
    num::{ParseFloatError, ParseIntError},
    str::FromStr,
};

use quick_xml::{encoding::EncodingError, events::Event};
use thiserror::Error;

#[derive(Debug, Error)]
/// Error produced when reading XML.
pub enum XmlReadError {
    #[error("{0}")]
    /// Failed to parse XML.
    Xml(#[from] quick_xml::Error),
    #[error("{0}")]
    /// Failed to decode XML content.
    Encoding(#[from] EncodingError),
    #[error("Unknown XML entity reference: &{0};")]
    /// Unknown XML entity reference.
    UnknownEntityReference(String),
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
                        text.push_str(&e.xml10_content()?);
                    } else if e.inplace_trim_start() {
                        continue;
                    } else {
                        text = Some(e.xml10_content()?.into_owned());
                    }
                }
                Event::GeneralRef(r) => {
                    if depth != 1 {
                        continue;
                    }
                    let reference = resolve_general_ref(&r)?;
                    text.get_or_insert_with(String::new).push(reference);
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
                Event::GeneralRef(r) => {
                    out.push(b'&');
                    out.extend_from_slice(&r);
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

fn resolve_general_ref(reference: &quick_xml::events::BytesRef<'_>) -> Result<char, XmlReadError> {
    if let Some(ch) = reference.resolve_char_ref()? {
        return Ok(ch);
    }

    match reference.decode()?.as_ref() {
        "amp" => Ok('&'),
        "lt" => Ok('<'),
        "gt" => Ok('>'),
        "quot" => Ok('"'),
        "apos" => Ok('\''),
        name => Err(XmlReadError::UnknownEntityReference(name.to_owned())),
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
            Hello <!-- Comment -->there
        </Foo>"#;

        let mut cursor = Cursor::new(xml.as_bytes());
        let mut reader = super::XmlStreamReader::new(&mut cursor);

        assert!(matches!(reader.next_event().unwrap(), Event::Start(_)));
        assert_eq!(reader.consume_as_text().unwrap(), "Hello there");
    }

    #[test]
    fn test_consume_as_text_resolves_general_refs() {
        let xml = r#"<Foo>&amp;&lt;&gt;&quot;&apos;&#49;&#x30; value</Foo>"#;

        let mut cursor = Cursor::new(xml.as_bytes());
        let mut reader = super::XmlStreamReader::new(&mut cursor);

        assert!(matches!(reader.next_event().unwrap(), Event::Start(_)));
        assert_eq!(reader.consume_as_text().unwrap(), "&<>\"'10 value");
    }

    #[test]
    fn test_consume_as_text_rejects_unknown_general_refs() {
        let xml = r#"<Foo>&custom;</Foo>"#;

        let mut cursor = Cursor::new(xml.as_bytes());
        let mut reader = super::XmlStreamReader::new(&mut cursor);

        assert!(matches!(reader.next_event().unwrap(), Event::Start(_)));
        assert!(matches!(
            reader.consume_as_text(),
            Err(super::XmlReadError::UnknownEntityReference(name)) if name == "custom"
        ));
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
        let xml = r#"<Foo>a &amp; b &#49;</Foo>"#;

        let mut cursor = Cursor::new(xml.as_bytes());
        let mut reader = super::XmlStreamReader::new(&mut cursor);

        assert!(matches!(reader.next_event().unwrap(), Event::Start(_)));
        assert_eq!(
            std::str::from_utf8(&reader.consume_raw().unwrap()).unwrap(),
            "a &amp; b &#49;"
        );
    }
}

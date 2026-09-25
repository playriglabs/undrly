//! Minimal XLSX (Office Open XML spreadsheet) reader for source workbooks.
//!
//! Reads one worksheet by name and returns every cell's value **exactly as
//! stored in the file**: shared and inline strings as text, numbers as the
//! literal `<v>` text (e.g. `1.1000000000000001`), never parsed through a
//! float. Styles, formulas and dates are not interpreted; interpreting
//! values is normalization.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

/// One cell's stored value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    /// A shared, inline or formula string.
    Text(String),
    /// A number, as the literal text stored in the sheet.
    Number(String),
    /// A boolean (`0`/`1`), error or other typed value, verbatim.
    Other(String),
}

impl Cell {
    /// The value's text regardless of type.
    pub fn text(&self) -> &str {
        match self {
            Cell::Text(s) | Cell::Number(s) | Cell::Other(s) => s,
        }
    }
}

/// A worksheet row: 1-based row number and cells keyed by column letters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub number: u32,
    pub cells: BTreeMap<String, Cell>,
}

impl Row {
    pub fn get(&self, column: &str) -> Option<&Cell> {
        self.cells.get(column)
    }

    /// The trimmed text of `column`, if present and non-empty.
    pub fn text(&self, column: &str) -> Option<&str> {
        self.get(column)
            .map(|c| c.text().trim())
            .filter(|s| !s.is_empty())
    }
}

const MAX_PART_BYTES: u64 = 64 * 1024 * 1024;

/// The rows of worksheet `sheet` that contain at least one value, in order.
pub fn read_sheet(workbook: &[u8], sheet: &str) -> Result<Vec<Row>, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(workbook)).map_err(|e| e.to_string())?;
    let mut part = |name: &str| -> Result<Option<Vec<u8>>, String> {
        let mut file = match zip.by_name(name) {
            Ok(f) => f,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(e) => return Err(e.to_string()),
        };
        if file.size() > MAX_PART_BYTES {
            return Err(format!("{name} is too large"));
        }
        let mut out = Vec::new();
        file.read_to_end(&mut out).map_err(|e| e.to_string())?;
        Ok(Some(out))
    };
    let book = part("xl/workbook.xml")?.ok_or("missing xl/workbook.xml")?;
    let rel_id = sheet_rel_id(&book, sheet)?.ok_or_else(|| format!("no sheet `{sheet}`"))?;
    let rels = part("xl/_rels/workbook.xml.rels")?.ok_or("missing workbook relationships")?;
    let target = rel_target(&rels, &rel_id)?.ok_or_else(|| format!("no target for {rel_id}"))?;
    let path = match target.strip_prefix('/') {
        Some(absolute) => absolute.to_owned(),
        None => format!("xl/{target}"),
    };
    let shared = match part("xl/sharedStrings.xml")? {
        Some(bytes) => shared_strings(&bytes)?,
        None => Vec::new(),
    };
    let sheet_xml = part(&path)?.ok_or_else(|| format!("missing {path}"))?;
    rows(&sheet_xml, &shared)
}

fn attr(e: &BytesStart<'_>, name: &[u8]) -> Result<Option<String>, String> {
    for a in e.attributes() {
        let a = a.map_err(|e| e.to_string())?;
        if a.key.as_ref() == name || a.key.local_name().as_ref() == name {
            return Ok(Some(
                a.unescape_value().map_err(|e| e.to_string())?.into_owned(),
            ));
        }
    }
    Ok(None)
}

/// Element attributes must match exactly on the qualified name (e.g.
/// `r:id`), so relationship ids are looked up by their qualified key.
fn attr_exact(e: &BytesStart<'_>, name: &[u8]) -> Result<Option<String>, String> {
    for a in e.attributes() {
        let a = a.map_err(|e| e.to_string())?;
        if a.key.as_ref() == name {
            return Ok(Some(
                a.unescape_value().map_err(|e| e.to_string())?.into_owned(),
            ));
        }
    }
    Ok(None)
}

fn sheet_rel_id(book: &[u8], sheet: &str) -> Result<Option<String>, String> {
    let mut reader = Reader::from_reader(book);
    let mut buf = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| e.to_string())?
        {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"sheet" => {
                if attr(&e, b"name")?.as_deref() == Some(sheet) {
                    return attr_exact(&e, b"r:id");
                }
            }
            Event::Eof => return Ok(None),
            _ => {}
        }
        buf.clear();
    }
}

fn rel_target(rels: &[u8], id: &str) -> Result<Option<String>, String> {
    let mut reader = Reader::from_reader(rels);
    let mut buf = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| e.to_string())?
        {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"Relationship" => {
                if attr(&e, b"Id")?.as_deref() == Some(id) {
                    return attr(&e, b"Target");
                }
            }
            Event::Eof => return Ok(None),
            _ => {}
        }
        buf.clear();
    }
}

/// Collects the text of `<t>` elements (skipping phonetic runs `<rPh>`)
/// until the end of the element named `end`.
fn text_until(reader: &mut Reader<&[u8]>, end: &[u8]) -> Result<String, String> {
    let mut out = String::new();
    let mut in_t = false;
    let mut in_phonetic = false;
    let mut buf = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| e.to_string())?
        {
            Event::Start(e) => match e.local_name().as_ref() {
                b"t" => in_t = true,
                b"rPh" => in_phonetic = true,
                _ => {}
            },
            Event::End(e) => match e.local_name().as_ref() {
                b"t" => in_t = false,
                b"rPh" => in_phonetic = false,
                name if name == end => return Ok(out),
                _ => {}
            },
            Event::Text(t) if in_t && !in_phonetic => {
                out.push_str(&t.unescape().map_err(|e| e.to_string())?);
            }
            Event::CData(t) if in_t && !in_phonetic => {
                out.push_str(&String::from_utf8_lossy(&t));
            }
            Event::Eof => return Err("unexpected end of document".into()),
            _ => {}
        }
        buf.clear();
    }
}

fn shared_strings(xml: &[u8]) -> Result<Vec<String>, String> {
    let mut reader = Reader::from_reader(xml);
    let mut out = Vec::new();
    let mut buf = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| e.to_string())?
        {
            Event::Start(e) if e.local_name().as_ref() == b"si" => {
                out.push(text_until(&mut reader, b"si")?);
            }
            Event::Empty(e) if e.local_name().as_ref() == b"si" => out.push(String::new()),
            Event::Eof => return Ok(out),
            _ => {}
        }
        buf.clear();
    }
}

fn split_ref(r: &str) -> Option<(String, u32)> {
    let letters: String = r.chars().take_while(char::is_ascii_uppercase).collect();
    let number = r[letters.len()..].parse().ok()?;
    (!letters.is_empty()).then_some((letters, number))
}

fn rows(xml: &[u8], shared: &[String]) -> Result<Vec<Row>, String> {
    let mut reader = Reader::from_reader(xml);
    let mut out: Vec<Row> = Vec::new();
    let mut buf = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| e.to_string())?
        {
            Event::Start(e) if e.local_name().as_ref() == b"c" => {
                let reference = attr(&e, b"r")?.ok_or("cell without a reference")?;
                let (column, number) = split_ref(&reference)
                    .ok_or_else(|| format!("bad cell reference `{reference}`"))?;
                let kind = attr(&e, b"t")?.unwrap_or_else(|| "n".to_owned());
                let cell = cell(&mut reader, &kind, shared)?;
                let Some(cell) = cell else { continue };
                match out.last_mut() {
                    Some(row) if row.number == number => {
                        row.cells.insert(column, cell);
                    }
                    Some(row) if row.number > number => {
                        return Err(format!("cell {reference} is out of order"));
                    }
                    _ => out.push(Row {
                        number,
                        cells: BTreeMap::from([(column, cell)]),
                    }),
                }
            }
            Event::Eof => return Ok(out),
            _ => {}
        }
        buf.clear();
    }
}

/// Reads a `<c>` element's content (after its start tag).
fn cell(reader: &mut Reader<&[u8]>, kind: &str, shared: &[String]) -> Result<Option<Cell>, String> {
    let mut value: Option<String> = None;
    let mut inline: Option<String> = None;
    let mut buf = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| e.to_string())?
        {
            Event::Start(e) if e.local_name().as_ref() == b"v" => {
                let mut text = String::new();
                let mut inner = Vec::new();
                loop {
                    match reader
                        .read_event_into(&mut inner)
                        .map_err(|e| e.to_string())?
                    {
                        Event::Text(t) => text.push_str(&t.unescape().map_err(|e| e.to_string())?),
                        Event::End(_) => break,
                        Event::Eof => return Err("unexpected end of document".into()),
                        _ => {}
                    }
                    inner.clear();
                }
                value = Some(text);
            }
            Event::Start(e) if e.local_name().as_ref() == b"is" => {
                inline = Some(text_until(reader, b"is")?);
            }
            Event::End(e) if e.local_name().as_ref() == b"c" => break,
            Event::Eof => return Err("unexpected end of document".into()),
            _ => {}
        }
        buf.clear();
    }
    Ok(match (kind, value, inline) {
        ("inlineStr", _, Some(s)) => Some(Cell::Text(s)),
        ("s", Some(v), _) => {
            let i: usize = v
                .trim()
                .parse()
                .map_err(|_| format!("bad shared string index `{v}`"))?;
            Some(Cell::Text(
                shared
                    .get(i)
                    .cloned()
                    .ok_or_else(|| format!("no shared string {i}"))?,
            ))
        }
        ("str", Some(v), _) => Some(Cell::Text(v)),
        ("n", Some(v), _) => Some(Cell::Number(v)),
        (_, Some(v), _) => Some(Cell::Other(v)),
        (_, None, _) => None,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;

    use super::*;

    /// Builds a small workbook in memory (synthetic; for tests only).
    pub(crate) fn workbook(sheets: &[(&str, &str)], shared: &[&str]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut out);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let mut book = String::from(
            r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>"#,
        );
        let mut rels = String::from(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        );
        for (i, (name, _)) in sheets.iter().enumerate() {
            book.push_str(&format!(
                r#"<sheet name="{name}" sheetId="{n}" r:id="rId{n}"/>"#,
                n = i + 1
            ));
            rels.push_str(&format!(
                r#"<Relationship Id="rId{n}" Type="worksheet" Target="worksheets/sheet{n}.xml"/>"#,
                n = i + 1
            ));
        }
        book.push_str("</sheets></workbook>");
        rels.push_str("</Relationships>");
        let mut put = |name: &str, body: &str| {
            zip.start_file(name, options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        };
        put("xl/workbook.xml", &book);
        put("xl/_rels/workbook.xml.rels", &rels);
        let strings: String = shared
            .iter()
            .map(|s| format!("<si><t>{s}</t></si>"))
            .collect();
        put(
            "xl/sharedStrings.xml",
            &format!(
                r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">{strings}</sst>"#
            ),
        );
        for (i, (_, data)) in sheets.iter().enumerate() {
            put(
                &format!("xl/worksheets/sheet{}.xml", i + 1),
                &format!(
                    r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{data}</sheetData></worksheet>"#
                ),
            );
        }
        zip.finish().unwrap();
        out.into_inner()
    }

    #[test]
    fn reads_cells_verbatim() {
        let book = workbook(
            &[
                ("Other", r#"<row r="1"><c r="A1"><v>9</v></c></row>"#),
                (
                    "Data",
                    r#"<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1"><v>1.1000000000000001</v></c></row>
                       <row r="3"><c r="A3" t="inlineStr"><is><t>R&amp;D</t></is></c><c r="C3" t="s"/><c r="D3"><v>6.66E-2</v></c></row>"#,
                ),
            ],
            &["Name"],
        );
        let rows = read_sheet(&book, "Data").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get("A"), Some(&Cell::Text("Name".into())));
        assert_eq!(
            rows[0].get("B"),
            Some(&Cell::Number("1.1000000000000001".into()))
        );
        assert_eq!(rows[1].number, 3);
        assert_eq!(rows[1].text("A"), Some("R&D"));
        assert_eq!(rows[1].get("C"), None);
        assert_eq!(rows[1].text("D"), Some("6.66E-2"));
        assert!(read_sheet(&book, "Missing").is_err());
        assert!(read_sheet(b"not a zip", "Data").is_err());
    }
}

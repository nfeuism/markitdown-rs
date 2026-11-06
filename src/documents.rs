//! Pure-Rust document -> Markdown converters.
//!
//! OOXML files (docx/xlsx/pptx) are ZIP containers of XML; we read them with
//! `zip` + `quick-xml`. PDF text layers come from `pdf-extract`. No Python,
//! no LibreOffice, no external process for the common path.

use std::io::{Cursor, Read};

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use zip::ZipArchive;

// ---------------------------------------------------------------------------
// Small XML event model
// ---------------------------------------------------------------------------

enum XmlEvent {
    Start { name: String, attrs: Vec<(String, String)> },
    End(String),
    Text(String),
}

fn parse_xml(xml: &[u8]) -> Result<Vec<XmlEvent>, String> {
    let mut reader = Reader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut events = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                events.push(XmlEvent::Start {
                    name: String::from_utf8_lossy(e.name().as_ref()).into_owned(),
                    attrs: read_attrs(&e),
                });
            }
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                events.push(XmlEvent::Start {
                    name: name.clone(),
                    attrs: read_attrs(&e),
                });
                events.push(XmlEvent::End(name));
            }
            Ok(Event::End(e)) => {
                events.push(XmlEvent::End(
                    String::from_utf8_lossy(e.name().as_ref()).into_owned(),
                ));
            }
            Ok(Event::Text(e)) => events.push(XmlEvent::Text(
                e.unescape().unwrap_or_default().into_owned(),
            )),
            Ok(Event::CData(e)) => events.push(XmlEvent::Text(
                String::from_utf8_lossy(&e).into_owned(),
            )),
            Ok(Event::Eof) => break,
            Err(err) => return Err(format!("XML 解析失败: {err}")),
            _ => {}
        }
        buf.clear();
    }
    Ok(events)
}

fn read_attrs(e: &quick_xml::events::BytesStart<'_>) -> Vec<(String, String)> {
    e.attributes()
        .filter_map(|a| a.ok())
        .map(|a| {
            (
                String::from_utf8_lossy(a.key.as_ref()).into_owned(),
                a.unescape_value().unwrap_or_default().into_owned(),
            )
        })
        .collect()
}

fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == key || k.rsplit(':').next() == Some(key))
        .map(|(_, v)| v.as_str())
}

fn collapse(text: &str) -> String {
    let mut out = String::new();
    let mut blanks = 0;
    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() {
            blanks += 1;
            if blanks > 1 {
                continue;
            }
        } else {
            blanks = 0;
        }
        out.push_str(trimmed);
        out.push('\n');
    }
    out.trim().to_string()
}

// ---------------------------------------------------------------------------
// ZIP / OOXML helpers
// ---------------------------------------------------------------------------

fn zip_names(bytes: &[u8]) -> Result<Vec<String>, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| format!("不是有效的 OOXML/ZIP 文件: {e}"))?;
    let mut names = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        if let Ok(file) = archive.by_index(i) {
            names.push(file.name().to_string());
        }
    }
    Ok(names)
}

fn zip_read(bytes: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| format!("不是有效的 OOXML/ZIP 文件: {e}"))?;
    let mut file = archive
        .by_name(name)
        .map_err(|e| format!("压缩包缺少 {name}: {e}"))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)
        .map_err(|e| format!("读取 {name} 失败: {e}"))?;
    Ok(buf)
}

fn trailing_number(path: &str) -> u32 {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

fn column_index(reference: &str) -> usize {
    let mut n = 0usize;
    for c in reference.chars().take_while(|c| c.is_ascii_alphabetic()) {
        n = n * 26 + (c.to_ascii_uppercase() as usize - 'A' as usize + 1);
    }
    n.saturating_sub(1)
}

fn heading_level(value: &str) -> Option<usize> {
    let lower = value.to_ascii_lowercase();
    if !lower.contains("heading") && !lower.contains("标题") {
        return None;
    }
    let digits: String = lower.chars().filter(|c| c.is_ascii_digit()).collect();
    let level: usize = digits.parse().ok()?;
    (1..=6).contains(&level).then_some(level)
}

pub fn render_table(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return "_（空表格）_".to_string();
    }
    let width = rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
    let normalize = |row: &Vec<String>| -> Vec<String> {
        let mut cells = row.clone();
        cells.resize(width, String::new());
        cells
            .iter()
            .map(|c| c.replace('|', "\\|").replace('\n', " ").trim().to_string())
            .collect()
    };
    let separator = vec!["---"; width].join(" | ");
    let mut out = format!(
        "| {} |\n| {} |\n",
        normalize(&rows[0]).join(" | "),
        separator
    );
    for row in &rows[1..] {
        out.push_str(&format!("| {} |\n", normalize(row).join(" | ")));
    }
    out.trim_end().to_string()
}

// ---------------------------------------------------------------------------
// Plain text / data / HTML
// ---------------------------------------------------------------------------

pub fn plain_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

pub fn fenced(text: &str, language: &str) -> String {
    format!("```{language}\n{}\n```", text.trim_end())
}

pub fn csv_to_markdown(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let first_line = text.lines().next().unwrap_or("");
    let delimiter = [',', '\t', ';']
        .into_iter()
        .max_by_key(|d| first_line.matches(*d).count())
        .unwrap_or(',');
    render_table(&parse_delimited(&text, delimiter))
}

fn parse_delimited(text: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' && field.is_empty() {
            quoted = true;
        } else if c == delimiter {
            row.push(std::mem::take(&mut field));
        } else if c == '\n' {
            row.push(std::mem::take(&mut field));
            if row.iter().any(|f| !f.trim().is_empty()) {
                rows.push(std::mem::take(&mut row));
            } else {
                row.clear();
            }
        } else if c != '\r' {
            field.push(c);
        }
    }
    row.push(field);
    if row.iter().any(|f| !f.trim().is_empty()) {
        rows.push(row);
    }
    rows
}

pub fn html_to_markdown(bytes: &[u8]) -> String {
    let source = String::from_utf8_lossy(bytes);
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::new();
    let mut skip_depth = 0usize;
    let mut hrefs: Vec<String> = Vec::new();
    let mut i = 0usize;

    while i < chars.len() {
        let c = chars[i];
        if c == '<' {
            let mut j = i + 1;
            let mut quote = '\0';
            while j < chars.len() {
                let d = chars[j];
                if quote != '\0' {
                    if d == quote {
                        quote = '\0';
                    }
                } else if d == '"' || d == '\'' {
                    quote = d;
                } else if d == '>' {
                    break;
                }
                j += 1;
            }
            let inner: String = chars[i + 1..j.min(chars.len())].iter().collect();
            i = j.saturating_add(1);

            let trimmed = inner.trim_start();
            let closing = trimmed.starts_with('/');
            let body = if closing { &trimmed[1..] } else { trimmed };
            let name = body
                .split(|ch: char| ch.is_whitespace() || ch == '/')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();

            match name.as_str() {
                "script" | "style" | "head" | "noscript" | "svg" | "nav" => {
                    if closing {
                        skip_depth = skip_depth.saturating_sub(1);
                    } else {
                        skip_depth += 1;
                    }
                }
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    if skip_depth == 0 {
                        if closing {
                            out.push_str("\n\n");
                        } else {
                            let level = name.as_bytes()[1] - b'0';
                            out.push_str(&format!("\n\n{} ", "#".repeat(level as usize)));
                        }
                    }
                }
                "p" | "div" | "section" | "article" | "header" | "footer" | "blockquote"
                | "table" | "ul" | "ol" | "tr" => {
                    if skip_depth == 0 {
                        out.push('\n');
                    }
                }
                "li" => {
                    if skip_depth == 0 {
                        out.push_str(if closing { "\n" } else { "\n- " });
                    }
                }
                "br" | "hr" => {
                    if skip_depth == 0 {
                        out.push('\n');
                    }
                }
                "td" | "th" => {
                    if skip_depth == 0 {
                        out.push_str(if closing { " |" } else { " | " });
                    }
                }
                "strong" | "b" | "em" | "i" => {
                    if skip_depth == 0 {
                        out.push_str(if matches!(name.as_str(), "strong" | "b") {
                            "**"
                        } else {
                            "*"
                        });
                    }
                }
                "code" | "pre" => {
                    if skip_depth == 0 {
                        out.push_str(if name == "pre" {
                            if closing { "\n```\n\n" } else { "\n\n```\n" }
                        } else {
                            "`"
                        });
                    }
                }
                "a" => {
                    if skip_depth == 0 {
                        if closing {
                            let href = hrefs.pop().unwrap_or_default();
                            out.push_str(&format!("]({href})"));
                        } else {
                            out.push('[');
                            hrefs.push(extract_attribute(&inner, "href"));
                        }
                    }
                }
                "img" => {
                    if skip_depth == 0 {
                        out.push_str(&format!(
                            "\n![{}]({})\n",
                            extract_attribute(&inner, "alt"),
                            extract_attribute(&inner, "src")
                        ));
                    }
                }
                _ => {}
            }
            continue;
        }

        if skip_depth > 0 {
            if c == '\n' {
                out.push('\n');
            }
            i += 1;
            continue;
        }

        if c == '&' {
            let mut k = i + 1;
            while k < chars.len() && k - i < 12 && chars[k] != ';' && chars[k] != '<' {
                k += 1;
            }
            if k < chars.len() && chars[k] == ';' {
                let entity: String = chars[i + 1..k].iter().collect();
                out.push_str(&decode_entity(&entity));
                i = k + 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    collapse(&out)
}

fn extract_attribute(tag: &str, key: &str) -> String {
    let lower = tag.to_ascii_lowercase();
    let needle = format!("{key}=");
    if let Some(pos) = lower.find(&needle) {
        let rest = &tag[pos + needle.len()..];
        let quote = rest.chars().next().unwrap_or(' ');
        if quote == '"' || quote == '\'' {
            return rest[1..].split(quote).next().unwrap_or("").to_string();
        }
        return rest
            .split(|c: char| c.is_whitespace() || c == '>')
            .next()
            .unwrap_or("")
            .to_string();
    }
    String::new()
}

fn decode_entity(entity: &str) -> String {
    match entity {
        "amp" => "&".to_string(),
        "lt" => "<".to_string(),
        "gt" => ">".to_string(),
        "quot" => "\"".to_string(),
        "apos" | "#39" | "#x27" => "'".to_string(),
        "nbsp" | "#160" => " ".to_string(),
        _ => {
            if let Some(hex) = entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| format!("&{entity};"))
            } else if let Some(dec) = entity.strip_prefix('#') {
                dec.parse::<u32>()
                    .ok()
                    .and_then(char::from_u32)
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| format!("&{entity};"))
            } else {
                format!("&{entity};")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PDF (text layer only; scanned PDFs are handled by the OCR module)
// ---------------------------------------------------------------------------

/// Does this PDF contain a Type0 font whose `/Encoding` is a Name other than
/// `Identity-H` (e.g. `UniGB-UCS2-H`, the standard Chinese invoice encoding)?
///
/// `pdf-extract` 0.7 hard-asserts `name == "Identity-H"` for exactly that
/// case, so detecting it up front lets us skip the text extractor and go
/// straight to OCR instead of provoking (and catching) a panic.
pub fn pdf_has_unsupported_encoding(bytes: &[u8]) -> bool {
    let Ok(doc) = lopdf::Document::load_mem(bytes) else {
        return false; // let the text extractor decide; catch_unwind still guards it
    };
    for object in doc.objects.values() {
        let Ok(dict) = object.as_dict() else { continue };
        let is_type0 = dict
            .get(b"Subtype")
            .ok()
            .and_then(|o| o.as_name().ok())
            .is_some_and(|name| name == b"Type0");
        if !is_type0 {
            continue;
        }
        if let Ok(encoding) = dict.get(b"Encoding") {
            if let Ok(name) = encoding.as_name() {
                if name != b"Identity-H" {
                    return true;
                }
            }
        }
    }
    false
}

/// Extract a PDF's text layer.
///
/// `pdf-extract` 0.7 asserts `name == "Identity-H"` for CID fonts, so plenty of
/// perfectly valid real-world PDFs make it panic. We isolate that panic here:
/// the caller then falls back to page rendering + OCR (or MarkItDown) instead
/// of the whole server dying.
pub fn pdf_text(bytes: &[u8]) -> Result<String, String> {
    if pdf_has_unsupported_encoding(bytes) {
        return Err("PDF 使用 pdf-extract 不支持的中文字体编码（如 UniGB-UCS2-H）".to_string());
    }
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pdf_extract::extract_text_from_mem(bytes)
    }));
    match outcome {
        Ok(Ok(text)) => Ok(text),
        Ok(Err(err)) => Err(format!("pdf-extract 无法解析: {err}")),
        Err(_) => Err("pdf-extract 遇到不支持的字体编码（内部断言）".to_string()),
    }
}

// ---------------------------------------------------------------------------
// DOCX
// ---------------------------------------------------------------------------

pub fn docx_to_markdown(bytes: &[u8]) -> Result<String, String> {
    let xml = zip_read(bytes, "word/document.xml")?;
    let events = parse_xml(&xml)?;
    let mut out = String::new();
    let mut paragraph = String::new();
    let mut heading = 0usize;
    let mut in_text = false;
    let mut cell_depth = 0usize;

    for event in events {
        match event {
            XmlEvent::Start { name, attrs } => match name.as_str() {
                "w:p" => {
                    paragraph.clear();
                    heading = 0;
                }
                "w:pStyle" => {
                    if let Some(value) = attr(&attrs, "w:val") {
                        heading = heading_level(value).unwrap_or(0);
                    }
                }
                "w:t" => in_text = true,
                "w:tab" => paragraph.push('\t'),
                "w:br" => paragraph.push('\n'),
                "w:tc" => {
                    cell_depth += 1;
                    paragraph.push_str(" | ");
                }
                _ => {}
            },
            XmlEvent::Text(text) => {
                if in_text {
                    paragraph.push_str(&text);
                }
            }
            XmlEvent::End(name) => match name.as_str() {
                "w:t" => in_text = false,
                "w:tc" => cell_depth = cell_depth.saturating_sub(1),
                "w:p" if cell_depth == 0 => {
                    let line = paragraph.trim();
                    if !line.is_empty() {
                        if heading > 0 {
                            out.push_str(&format!("{} {line}\n\n", "#".repeat(heading)));
                        } else {
                            out.push_str(line);
                            out.push_str("\n\n");
                        }
                    }
                    paragraph.clear();
                    heading = 0;
                }
                "w:tr" => {
                    let line = paragraph.trim();
                    if !line.is_empty() {
                        out.push_str(line);
                        out.push('\n');
                    }
                    paragraph.clear();
                    heading = 0;
                }
                _ => {}
            },
        }
    }
    Ok(collapse(&out))
}

// ---------------------------------------------------------------------------
// XLSX
// ---------------------------------------------------------------------------

fn shared_strings(bytes: &[u8]) -> Vec<String> {
    let Ok(xml) = zip_read(bytes, "xl/sharedStrings.xml") else {
        return Vec::new();
    };
    let Ok(events) = parse_xml(&xml) else {
        return Vec::new();
    };
    let mut strings = Vec::new();
    let mut current = String::new();
    let mut in_item = false;
    let mut in_text = false;
    for event in events {
        match event {
            XmlEvent::Start { name, .. } if name == "si" => {
                in_item = true;
                current.clear();
            }
            XmlEvent::Start { name, .. } if name == "t" && in_item => in_text = true,
            XmlEvent::Text(text) if in_text => current.push_str(&text),
            XmlEvent::End(name) if name == "t" => in_text = false,
            XmlEvent::End(name) if name == "si" => {
                in_item = false;
                strings.push(std::mem::take(&mut current));
            }
            _ => {}
        }
    }
    strings
}

fn parse_sheet(xml: &[u8], shared: &[String]) -> Result<Vec<Vec<String>>, String> {
    let events = parse_xml(xml)?;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<(usize, String)> = Vec::new();
    let mut column = 0usize;
    let mut cell_type = String::new();
    let mut value = String::new();
    let mut in_value = false;
    let mut in_inline_text = false;

    for event in events {
        match event {
            XmlEvent::Start { name, attrs } => match name.as_str() {
                "c" => {
                    column = column_index(attr(&attrs, "r").unwrap_or("A1"));
                    cell_type = attr(&attrs, "t").unwrap_or("").to_string();
                    value.clear();
                }
                "v" => in_value = true,
                "is" => in_inline_text = true,
                _ => {}
            },
            XmlEvent::Text(text) => {
                if in_value || in_inline_text {
                    value.push_str(&text);
                }
            }
            XmlEvent::End(name) => match name.as_str() {
                "v" => in_value = false,
                "is" => in_inline_text = false,
                "c" => {
                    let text = if cell_type == "s" {
                        value
                            .trim()
                            .parse::<usize>()
                            .ok()
                            .and_then(|i| shared.get(i).cloned())
                            .unwrap_or_default()
                    } else {
                        value.clone()
                    };
                    if !text.trim().is_empty() {
                        row.push((column, text));
                    }
                }
                "row" => {
                    if !row.is_empty() {
                        let width = row.iter().map(|(c, _)| *c).max().unwrap_or(0) + 1;
                        let mut cells = vec![String::new(); width];
                        for (c, text) in row.drain(..) {
                            cells[c] = text;
                        }
                        rows.push(cells);
                    } else {
                        row.clear();
                    }
                }
                _ => {}
            },
        }
    }
    Ok(rows)
}

pub fn xlsx_to_markdown(bytes: &[u8]) -> Result<String, String> {
    let shared = shared_strings(bytes);
    let mut sheets: Vec<String> = zip_names(bytes)?
        .into_iter()
        .filter(|p| p.starts_with("xl/worksheets/sheet") && p.ends_with(".xml"))
        .collect();
    sheets.sort_by_key(|p| trailing_number(p));
    if sheets.is_empty() {
        return Err("XLSX 中没有找到工作表".to_string());
    }
    let mut out = String::new();
    for sheet in sheets {
        let xml = zip_read(bytes, &sheet)?;
        let table = render_table(&parse_sheet(&xml, &shared)?);
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&table);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// PPTX
// ---------------------------------------------------------------------------

pub fn pptx_to_markdown(bytes: &[u8]) -> Result<String, String> {
    let mut slides: Vec<String> = zip_names(bytes)?
        .into_iter()
        .filter(|p| p.starts_with("ppt/slides/slide") && p.ends_with(".xml"))
        .collect();
    slides.sort_by_key(|p| trailing_number(p));
    if slides.is_empty() {
        return Err("PPTX 中没有找到幻灯片".to_string());
    }
    let mut out = String::new();
    for (index, slide) in slides.iter().enumerate() {
        let xml = zip_read(bytes, slide)?;
        let events = parse_xml(&xml)?;
        let mut lines = Vec::new();
        let mut current = String::new();
        let mut in_text = false;
        for event in events {
            match event {
                XmlEvent::Start { name, .. } if name == "a:t" => {
                    in_text = true;
                    current.clear();
                }
                XmlEvent::Text(text) if in_text => current.push_str(&text),
                XmlEvent::End(name) if name == "a:t" => {
                    in_text = false;
                    if !current.trim().is_empty() {
                        lines.push(current.trim().to_string());
                    }
                    current.clear();
                }
                _ => {}
            }
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&format!("## 幻灯片 {}\n\n", index + 1));
        out.push_str(&lines.join("\n\n"));
    }
    Ok(out)
}

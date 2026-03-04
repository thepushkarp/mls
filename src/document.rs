/// Document metadata extraction (PDF, Office, `OpenDocument`, plain text).
///
/// All extractors are best-effort: failures are silently swallowed and
/// logged at debug level. Follows the same pattern as `exif.rs`.
use crate::types::DocumentInfo;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

/// Maximum bytes to read from a single XML file inside a zip archive (4 MiB).
/// Prevents OOM from crafted archives with enormous embedded XML.
const MAX_XML_BYTES: u64 = 4 * 1024 * 1024;

/// Maximum bytes to scan from a text/CSV/TSV file (256 MiB).
/// Prevents scanning multi-GB log files or data dumps.
const MAX_TEXT_SCAN_BYTES: u64 = 256 * 1024 * 1024;

/// Extract metadata from a document file, dispatching by extension.
///
/// Returns `None` on any failure (corrupt file, unsupported format, I/O error).
pub fn probe_document(path: &Path) -> Option<DocumentInfo> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)?;

    let result = match ext.as_str() {
        "pdf" => probe_pdf(path),
        "docx" => probe_ooxml_doc(path),
        "xlsx" => probe_ooxml_spreadsheet(path),
        "pptx" => probe_ooxml_presentation(path),
        "odt" | "ods" | "odp" => probe_odf(path, &ext),
        "doc" | "xls" | "ppt" => probe_ole2(path, &ext),
        "csv" | "tsv" => probe_text_table(path, &ext),
        "txt" | "md" => probe_text(path, &ext),
        _ => None,
    };

    if result.is_none() {
        tracing::debug!(path = %path.display(), ext = %ext, "document probe returned no metadata");
    }

    result
}

// ─── PDF ────────────────────────────────────────────────────────────────

fn probe_pdf(path: &Path) -> Option<DocumentInfo> {
    let doc = lopdf::Document::load(path).ok()?;

    let page_count = doc.get_pages().len();

    let trailer_info = doc
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|obj| obj.as_reference().ok())
        .and_then(|r| doc.get_object(r).ok());

    let get_info_str = |key: &[u8]| -> Option<String> {
        trailer_info?
            .as_dict()
            .ok()?
            .get(key)
            .ok()
            .and_then(pdf_object_to_string)
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    };

    #[expect(clippy::cast_possible_truncation)]
    Some(DocumentInfo {
        format: "pdf".to_string(),
        page_count: Some(page_count as u32),
        word_count: None,
        line_count: None,
        sheet_count: None,
        author: get_info_str(b"Author"),
        title: get_info_str(b"Title"),
        subject: get_info_str(b"Subject"),
        creator_app: get_info_str(b"Creator"),
        creation_date: get_info_str(b"CreationDate"),
        modification_date: get_info_str(b"ModDate"),
    })
}

fn pdf_object_to_string(obj: &lopdf::Object) -> Option<String> {
    match obj {
        lopdf::Object::String(bytes, _) => String::from_utf8(bytes.clone()).ok(),
        lopdf::Object::Name(name) => String::from_utf8(name.clone()).ok(),
        _ => None,
    }
}

// ─── OOXML (DOCX/XLSX/PPTX) ────────────────────────────────────────────

fn read_xml_from_zip(path: &Path, inner_path: &str) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let entry = archive.by_name(inner_path).ok()?;
    let mut contents = String::new();
    entry.take(MAX_XML_BYTES).read_to_string(&mut contents).ok()?;
    Some(contents)
}

fn parse_ooxml_core(path: &Path) -> OoxmlCoreProps {
    let xml = read_xml_from_zip(path, "docProps/core.xml").unwrap_or_default();
    parse_core_xml(&xml)
}

fn parse_ooxml_app(path: &Path) -> OoxmlAppProps {
    let xml = read_xml_from_zip(path, "docProps/app.xml").unwrap_or_default();
    parse_app_xml(&xml)
}

struct OoxmlCoreProps {
    author: Option<String>,
    title: Option<String>,
    subject: Option<String>,
    created: Option<String>,
    modified: Option<String>,
}

struct OoxmlAppProps {
    pages: Option<u32>,
    words: Option<u64>,
    slides: Option<u32>,
    app_name: Option<String>,
}

fn parse_core_xml(xml: &str) -> OoxmlCoreProps {
    let mut props = OoxmlCoreProps {
        author: None,
        title: None,
        subject: None,
        created: None,
        modified: None,
    };

    parse_xml_text_fields(xml, |tag, val| match tag {
        "creator" => props.author = Some(val),
        "title" => props.title = Some(val),
        "subject" => props.subject = Some(val),
        "created" => props.created = Some(val),
        "modified" => props.modified = Some(val),
        _ => {}
    });

    props
}

fn parse_app_xml(xml: &str) -> OoxmlAppProps {
    let mut props = OoxmlAppProps {
        pages: None,
        words: None,
        slides: None,
        app_name: None,
    };

    parse_xml_text_fields(xml, |tag, val| match tag {
        "Pages" => props.pages = val.parse().ok(),
        "Words" => props.words = val.parse().ok(),
        "Slides" => props.slides = val.parse().ok(),
        "Application" => props.app_name = Some(val),
        _ => {}
    });

    props
}

fn probe_ooxml_doc(path: &Path) -> Option<DocumentInfo> {
    let file = std::fs::File::open(path).ok()?;
    let _archive = zip::ZipArchive::new(file).ok()?;

    let core = parse_ooxml_core(path);
    let app = parse_ooxml_app(path);

    Some(DocumentInfo {
        format: "docx".to_string(),
        page_count: app.pages,
        word_count: app.words,
        line_count: None,
        sheet_count: None,
        author: core.author,
        title: core.title,
        subject: core.subject,
        creator_app: app.app_name,
        creation_date: core.created,
        modification_date: core.modified,
    })
}

fn probe_ooxml_spreadsheet(path: &Path) -> Option<DocumentInfo> {
    let file = std::fs::File::open(path).ok()?;
    let _archive = zip::ZipArchive::new(file).ok()?;

    let core = parse_ooxml_core(path);
    let app = parse_ooxml_app(path);

    let sheet_count = read_xml_from_zip(path, "xl/workbook.xml")
        .map(|xml| count_xml_elements(&xml, "sheet"))
        .filter(|&c| c > 0);

    Some(DocumentInfo {
        format: "xlsx".to_string(),
        page_count: None,
        word_count: None,
        line_count: None,
        sheet_count,
        author: core.author,
        title: core.title,
        subject: core.subject,
        creator_app: app.app_name,
        creation_date: core.created,
        modification_date: core.modified,
    })
}

fn probe_ooxml_presentation(path: &Path) -> Option<DocumentInfo> {
    let file = std::fs::File::open(path).ok()?;
    let _archive = zip::ZipArchive::new(file).ok()?;

    let core = parse_ooxml_core(path);
    let app = parse_ooxml_app(path);

    let slide_count = app.slides.or_else(|| {
        read_xml_from_zip(path, "ppt/presentation.xml")
            .map(|xml| count_xml_elements(&xml, "sldId"))
            .filter(|&c| c > 0)
    });

    Some(DocumentInfo {
        format: "pptx".to_string(),
        page_count: slide_count,
        word_count: None,
        line_count: None,
        sheet_count: None,
        author: core.author,
        title: core.title,
        subject: core.subject,
        creator_app: app.app_name,
        creation_date: core.created,
        modification_date: core.modified,
    })
}

// ─── ODF (ODT/ODS/ODP) ─────────────────────────────────────────────────

fn probe_odf(path: &Path, ext: &str) -> Option<DocumentInfo> {
    let file = std::fs::File::open(path).ok()?;
    let _archive = zip::ZipArchive::new(file).ok()?;

    let meta_xml = read_xml_from_zip(path, "meta.xml").unwrap_or_default();
    let meta = parse_odf_meta(&meta_xml);

    let (page_count, sheet_count) = match ext {
        "ods" => {
            let content = read_xml_from_zip(path, "content.xml").unwrap_or_default();
            let sheets = count_xml_elements(&content, "table");
            (None, if sheets > 0 { Some(sheets) } else { None })
        }
        "odp" => {
            let content = read_xml_from_zip(path, "content.xml").unwrap_or_default();
            let slides = count_xml_elements(&content, "page");
            (if slides > 0 { Some(slides) } else { None }, None)
        }
        _ => (meta.page_count, None),
    };

    Some(DocumentInfo {
        format: ext.to_string(),
        page_count,
        word_count: meta.word_count,
        line_count: None,
        sheet_count,
        author: meta.author,
        title: meta.title,
        subject: meta.subject,
        creator_app: meta.generator,
        creation_date: meta.creation_date,
        modification_date: meta.modification_date,
    })
}

struct OdfMeta {
    author: Option<String>,
    title: Option<String>,
    subject: Option<String>,
    generator: Option<String>,
    creation_date: Option<String>,
    modification_date: Option<String>,
    page_count: Option<u32>,
    word_count: Option<u64>,
}

fn parse_odf_meta(xml: &str) -> OdfMeta {
    let mut meta = OdfMeta {
        author: None,
        title: None,
        subject: None,
        generator: None,
        creation_date: None,
        modification_date: None,
        page_count: None,
        word_count: None,
    };

    let mut reader = quick_xml::Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut current_tag = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e) | quick_xml::events::Event::Empty(ref e)) => {
                let local = local_name(e.name().as_ref());

                // ODF stores statistics as attributes on `meta:document-statistic`
                if local == "document-statistic" {
                    for attr in e.attributes().flatten() {
                        let key = local_name(attr.key.as_ref());
                        let val = String::from_utf8_lossy(&attr.value).to_string();
                        match key.as_str() {
                            "page-count" => meta.page_count = val.parse().ok(),
                            "word-count" => meta.word_count = val.parse().ok(),
                            _ => {}
                        }
                    }
                }

                current_tag = local;
            }
            Ok(quick_xml::events::Event::Text(ref e)) => {
                let text = e.unescape().ok().map(|s| s.trim().to_owned());
                if let Some(val) = text.filter(|s| !s.is_empty()) {
                    match current_tag.as_str() {
                        "initial-creator" | "creator" => meta.author = Some(val),
                        "title" => meta.title = Some(val),
                        "subject" => meta.subject = Some(val),
                        "generator" => meta.generator = Some(val),
                        "creation-date" => meta.creation_date = Some(val),
                        "date" => meta.modification_date = Some(val),
                        _ => {}
                    }
                }
            }
            Ok(quick_xml::events::Event::End(_)) => {
                current_tag.clear();
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    meta
}

// ─── OLE2 (legacy DOC/XLS/PPT) ─────────────────────────────────────────

fn probe_ole2(path: &Path, ext: &str) -> Option<DocumentInfo> {
    let mut comp = cfb::open(path).ok()?;

    let mut info = DocumentInfo {
        format: ext.to_string(),
        ..DocumentInfo::default()
    };

    // Try to read the SummaryInformation stream
    if let Ok(stream) = comp.open_stream("/\x05SummaryInformation") {
        let data: Vec<u8> = std::io::Read::bytes(stream)
            .take(4096)
            .filter_map(Result::ok)
            .collect();
        parse_summary_info(&data, &mut info);
    }

    Some(info)
}

// MS-OLEPS binary format constants
const OLEPS_BYTE_ORDER_LE: u16 = 0xFFFE;
const OLEPS_HEADER_MIN_LEN: usize = 48;
const OLEPS_SECTION_OFFSET_POS: usize = 44;
const OLEPS_MAX_PROPS: usize = 100;
const OLEPS_SECTION_HEADER_SIZE: usize = 8;
const OLEPS_PROP_ENTRY_SIZE: usize = 8;
const VT_I4: u32 = 0x03;
const VT_LPSTR: u32 = 0x1E;
// Property IDs from the Summary Information property set
const PIDSI_TITLE: u32 = 2;
const PIDSI_AUTHOR: u32 = 4;
const PIDSI_SUBJECT: u32 = 5;
const PIDSI_PAGECOUNT: u32 = 14;
const PIDSI_WORDCOUNT: u32 = 15;
const PIDSI_APPNAME: u32 = 18;

/// Best-effort extraction from OLE2 `SummaryInformation` stream.
///
/// The stream uses MS-OLEPS binary format with property sets.
fn parse_summary_info(data: &[u8], info: &mut DocumentInfo) {
    if data.len() < OLEPS_HEADER_MIN_LEN || read_u16_le(data, 0) != OLEPS_BYTE_ORDER_LE {
        return;
    }

    let section_offset = read_u32_le(data, OLEPS_SECTION_OFFSET_POS) as usize;
    if section_offset + OLEPS_SECTION_HEADER_SIZE > data.len() {
        return;
    }

    let prop_count = read_u32_le(data, section_offset + 4) as usize;
    if prop_count > OLEPS_MAX_PROPS {
        return;
    }

    for i in 0..prop_count {
        let entry_offset = section_offset + OLEPS_SECTION_HEADER_SIZE + i * OLEPS_PROP_ENTRY_SIZE;
        if entry_offset + OLEPS_PROP_ENTRY_SIZE > data.len() {
            break;
        }

        let prop_id = read_u32_le(data, entry_offset);
        let prop_offset = read_u32_le(data, entry_offset + 4) as usize;
        let abs_offset = section_offset + prop_offset;

        if abs_offset + OLEPS_SECTION_HEADER_SIZE > data.len() {
            continue;
        }

        let prop_type = read_u32_le(data, abs_offset);

        if prop_type == VT_LPSTR {
            let str_len = read_u32_le(data, abs_offset + 4) as usize;
            let str_start = abs_offset + OLEPS_SECTION_HEADER_SIZE;
            if str_start + str_len <= data.len() {
                let raw = &data[str_start..str_start + str_len];
                let s = String::from_utf8_lossy(raw)
                    .trim_end_matches('\0')
                    .trim()
                    .to_owned();
                if !s.is_empty() {
                    match prop_id {
                        PIDSI_TITLE => info.title = Some(s),
                        PIDSI_AUTHOR => info.author = Some(s),
                        PIDSI_SUBJECT => info.subject = Some(s),
                        PIDSI_APPNAME => info.creator_app = Some(s),
                        _ => {}
                    }
                }
            }
        }

        if prop_type == VT_I4 && abs_offset + OLEPS_SECTION_HEADER_SIZE <= data.len() {
            let val = read_u32_le(data, abs_offset + 4);
            if val > 0 {
                match prop_id {
                    PIDSI_PAGECOUNT => info.page_count = Some(val),
                    PIDSI_WORDCOUNT => info.word_count = Some(u64::from(val)),
                    _ => {}
                }
            }
        }
    }
}

fn read_u16_le(data: &[u8], offset: usize) -> u16 {
    if offset + 2 > data.len() {
        return 0;
    }
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    if offset + 4 > data.len() {
        return 0;
    }
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

// ─── Text-based formats (CSV/TSV/TXT/MD) ───────────────────────────────

fn probe_text_table(path: &Path, ext: &str) -> Option<DocumentInfo> {
    let file = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(file.take(MAX_TEXT_SCAN_BYTES));
    let line_count = reader
        .bytes()
        .filter(|b| b.as_ref().is_ok_and(|&c| c == b'\n'))
        .count() as u64;

    Some(DocumentInfo {
        format: ext.to_string(),
        line_count: Some(line_count),
        ..DocumentInfo::default()
    })
}

fn probe_text(path: &Path, ext: &str) -> Option<DocumentInfo> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = BufReader::new(file.take(MAX_TEXT_SCAN_BYTES));
    let mut line_count: u64 = 0;
    let mut word_count: u64 = 0;
    let mut buf = String::new();

    loop {
        buf.clear();
        match reader.read_line(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                line_count += 1;
                word_count += buf.split_whitespace().count() as u64;
            }
        }
    }

    Some(DocumentInfo {
        format: ext.to_string(),
        word_count: Some(word_count),
        line_count: Some(line_count),
        ..DocumentInfo::default()
    })
}

// ─── XML helpers ────────────────────────────────────────────────────────

/// Run the `quick_xml` event loop and call `on_field` for each tag-text pair.
///
/// Shared by `parse_core_xml` and `parse_app_xml` which differ only in
/// which tags they care about. Not used by `parse_odf_meta` which also
/// reads attributes.
fn parse_xml_text_fields(xml: &str, mut on_field: impl FnMut(&str, String)) {
    use quick_xml::events::Event;

    let mut reader = quick_xml::Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut current_tag = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e) | Event::Empty(ref e)) => {
                current_tag = local_name(e.name().as_ref());
            }
            Ok(Event::Text(ref e)) => {
                if let Some(val) = e
                    .unescape()
                    .ok()
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty())
                {
                    on_field(&current_tag, val);
                }
            }
            Ok(Event::End(_)) => current_tag.clear(),
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
}

/// Extract the local part of a possibly namespaced XML name.
///
/// Examples: `dc:creator` becomes `creator`, `meta:creation-date` becomes `creation-date`.
fn local_name(name: &[u8]) -> String {
    let full = String::from_utf8_lossy(name);
    full.rsplit_once(':')
        .map_or(full.to_string(), |(_, local)| local.to_string())
}

fn count_xml_elements(xml: &str, element_local_name: &str) -> u32 {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut count: u32 = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e) | quick_xml::events::Event::Empty(ref e)) => {
                let local = local_name(e.name().as_ref());
                if local == element_local_name {
                    count = count.saturating_add(1);
                }
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    count
}

#[cfg(test)]
#[expect(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn local_name_strips_namespace() {
        assert_eq!(local_name(b"dc:creator"), "creator");
        assert_eq!(local_name(b"meta:creation-date"), "creation-date");
        assert_eq!(local_name(b"title"), "title");
    }

    #[test]
    fn probe_text_counts_lines_and_words() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.txt");
        std::fs::write(&path, "hello world\nfoo bar baz\n").unwrap();

        let info = probe_text(&path, "txt").unwrap();
        assert_eq!(info.format, "txt");
        assert_eq!(info.line_count, Some(2));
        assert_eq!(info.word_count, Some(5));
    }

    #[test]
    fn probe_text_empty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("empty.txt");
        std::fs::write(&path, "").unwrap();

        let info = probe_text(&path, "txt").unwrap();
        assert_eq!(info.line_count, Some(0));
        assert_eq!(info.word_count, Some(0));
    }

    #[test]
    fn probe_text_table_counts_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("data.csv");
        std::fs::write(&path, "a,b,c\n1,2,3\n4,5,6\n").unwrap();

        let info = probe_text_table(&path, "csv").unwrap();
        assert_eq!(info.format, "csv");
        assert_eq!(info.line_count, Some(3));
    }

    #[test]
    fn probe_nonexistent_file_returns_none() {
        let path = Path::new("/nonexistent/file.pdf");
        assert!(probe_document(path).is_none());
    }

    #[test]
    fn probe_corrupt_pdf_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("bad.pdf");
        std::fs::write(&path, b"this is not a pdf").unwrap();

        assert!(probe_pdf(&path).is_none());
    }

    #[test]
    fn probe_corrupt_zip_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("bad.docx");
        std::fs::write(&path, b"not a zip file").unwrap();

        assert!(probe_ooxml_doc(&path).is_none());
    }

    #[test]
    fn parse_core_xml_extracts_fields() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <cp:coreProperties xmlns:dc="http://purl.org/dc/elements/1.1/"
            xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties"
            xmlns:dcterms="http://purl.org/dc/terms/">
            <dc:creator>Jane Doe</dc:creator>
            <dc:title>My Document</dc:title>
            <dc:subject>Testing</dc:subject>
            <dcterms:created>2024-01-15T10:30:00Z</dcterms:created>
            <dcterms:modified>2024-06-20T14:00:00Z</dcterms:modified>
        </cp:coreProperties>"#;

        let props = parse_core_xml(xml);
        assert_eq!(props.author.as_deref(), Some("Jane Doe"));
        assert_eq!(props.title.as_deref(), Some("My Document"));
        assert_eq!(props.subject.as_deref(), Some("Testing"));
        assert_eq!(props.created.as_deref(), Some("2024-01-15T10:30:00Z"));
        assert_eq!(props.modified.as_deref(), Some("2024-06-20T14:00:00Z"));
    }

    #[test]
    fn parse_app_xml_extracts_fields() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties">
            <Application>Microsoft Word</Application>
            <Pages>42</Pages>
            <Words>12500</Words>
        </Properties>"#;

        let props = parse_app_xml(xml);
        assert_eq!(props.app_name.as_deref(), Some("Microsoft Word"));
        assert_eq!(props.pages, Some(42));
        assert_eq!(props.words, Some(12500));
    }

    #[test]
    fn parse_odf_meta_extracts_fields() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <office:document-meta xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
            xmlns:dc="http://purl.org/dc/elements/1.1/"
            xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0">
            <office:meta>
                <meta:initial-creator>John Smith</meta:initial-creator>
                <dc:title>ODF Doc</dc:title>
                <dc:subject>Testing ODF</dc:subject>
                <meta:generator>LibreOffice/7.5</meta:generator>
                <meta:creation-date>2024-03-01T09:00:00</meta:creation-date>
                <dc:date>2024-03-15T12:00:00</dc:date>
                <meta:document-statistic meta:page-count="10" meta:word-count="2500"/>
            </office:meta>
        </office:document-meta>"#;

        let meta = parse_odf_meta(xml);
        assert_eq!(meta.author.as_deref(), Some("John Smith"));
        assert_eq!(meta.title.as_deref(), Some("ODF Doc"));
        assert_eq!(meta.subject.as_deref(), Some("Testing ODF"));
        assert_eq!(meta.generator.as_deref(), Some("LibreOffice/7.5"));
        assert_eq!(meta.page_count, Some(10));
        assert_eq!(meta.word_count, Some(2500));
    }

    #[test]
    fn count_xml_elements_counts_correctly() {
        let xml = r#"<root><sheet name="A"/><sheet name="B"/><other/><sheet name="C"/></root>"#;
        assert_eq!(count_xml_elements(xml, "sheet"), 3);
        assert_eq!(count_xml_elements(xml, "other"), 1);
        assert_eq!(count_xml_elements(xml, "missing"), 0);
    }

    #[test]
    fn summary_info_empty_data() {
        let mut info = DocumentInfo::default();
        parse_summary_info(&[], &mut info);
        assert!(info.title.is_none());
        assert!(info.author.is_none());
    }
}

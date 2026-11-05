//! Bounded Markdown creation through the same model/importer used by File > Open.
use std::sync::Arc;

use serde_json::{Value, json};
use wordcraft_doc::{SectionProps, props::LineSpacing};
use wordcraft_formats::model::{FBlock, Fmt, Inline, Kind, Para};

use crate::{CmdError, CmdResult, CommandSpec, Session};

pub const MAX_MARKDOWN: usize = 512 << 10;
pub const MAX_BLOCKS: usize = 4_000;
pub const MAX_CELLS: usize = 4_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec::new("document.create", "Create document from Markdown", "Agents", create)
        .params(r#"{"markdown": string, "title"?: string, "path"?: string (.docx), "page_size"?: "letter|a4", "font"?: string, "font_size"?: 9..16, "margin"?: 36..108 (points)}"#)
        .pure()]
}

fn string<'a>(v: &'a Value, key: &str) -> Result<Option<&'a str>, CmdError> {
    match v.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(CmdError::Params(format!("{key} must be a string"))),
    }
}

fn number(v: &Value, key: &str, default: f32, min: f32, max: f32) -> Result<f32, CmdError> {
    match v.get(key) {
        None => Ok(default),
        Some(n) => n
            .as_f64()
            .filter(|n| n.is_finite() && *n >= min as f64 && *n <= max as f64)
            .map(|n| n as f32)
            .ok_or_else(|| CmdError::Params(format!("{key} must be between {min} and {max}"))),
    }
}

/// Count before expanding flow blocks into the full document model.
fn limits(blocks: &[FBlock], depth: usize, count: &mut usize, cells: &mut usize) -> Result<(), CmdError> {
    if depth > 16 {
        return Err(CmdError::Params("content nesting exceeds 16".into()));
    }
    for b in blocks {
        *count = count.saturating_add(1);
        if *count > MAX_BLOCKS {
            return Err(CmdError::Params("content exceeds 4000 blocks".into()));
        }
        if let FBlock::Table(t) = b {
            if t.rows.len() > 200 || t.rows.iter().any(|r| r.len() > 16) {
                return Err(CmdError::Params("tables allow at most 200 rows and 16 columns".into()));
            }
            for r in &t.rows {
                for c in r {
                    *cells = cells.saturating_add(1);
                    if *cells > MAX_CELLS {
                        return Err(CmdError::Params("content exceeds 4000 table cells".into()));
                    }
                    limits(&c.blocks, depth + 1, count, cells)?;
                }
            }
        }
    }
    Ok(())
}

fn create(s: &mut Session, v: &Value) -> CmdResult {
    let obj = v.as_object().ok_or_else(|| CmdError::Params("document.create needs an object".into()))?;
    if obj.keys().any(|k| !["markdown", "title", "path", "page_size", "font", "font_size", "margin"].contains(&k.as_str())) {
        return Err(CmdError::Params("unsupported document.create setting".into()));
    }
    let markdown = string(v, "markdown")?.filter(|m| !m.trim().is_empty()).ok_or_else(|| CmdError::Params("markdown must be non-empty".into()))?;
    if markdown.len() > MAX_MARKDOWN || markdown.lines().count() > MAX_BLOCKS || markdown.lines().any(|l| l.len() > 8192) {
        return Err(CmdError::Params("Markdown allows 512 KiB, 4000 lines, and 8192 bytes per line".into()));
    }
    if markdown.chars().any(|c| (c.is_control() && !matches!(c, '\n' | '\r' | '\t')) || matches!(c, '\u{fffe}' | '\u{ffff}')) {
        return Err(CmdError::Params("Markdown contains invalid XML characters".into()));
    }
    // Before parsing: no embedded decoder work, unbounded nesting, or table expansion.
    if markdown.contains("![") {
        return Err(CmdError::Params("Markdown images are unsupported (image limit 0); insert pictures with insert.picture after creation".into()));
    }
    let mut depth = 0usize;
    for line in markdown.lines() {
        let indentation = line.chars().take_while(|c| c.is_whitespace()).map(|c| if c == '\t' { 4 } else { 1 }).sum::<usize>();
        if indentation > 64 || line.chars().take_while(|c| *c == '>' || c.is_whitespace()).filter(|c| *c == '>').count() > 16 {
            return Err(CmdError::Params("Markdown nesting exceeds 16".into()));
        }
        for c in line.chars() {
            if c == '[' {
                depth = depth.saturating_add(1);
            }
            if depth > 16 {
                return Err(CmdError::Params("inline nesting exceeds 16".into()));
            }
            if c == ']' {
                depth = depth.saturating_sub(1);
            }
        }
        if line.matches('|').count() > 18 {
            return Err(CmdError::Params("tables allow at most 16 columns".into()));
        }
    }
    let title = string(v, "title")?;
    if title.is_some_and(|t| t.len() > 512 || t.chars().any(|c| c.is_control() || matches!(c, '\u{fffe}' | '\u{ffff}'))) {
        return Err(CmdError::Params("title allows 512 bytes and no control characters".into()));
    }
    let requested = string(v, "path")?.unwrap_or("Document.docx");
    if !std::path::Path::new(requested).extension().is_some_and(|e| e.eq_ignore_ascii_case("docx")) {
        return Err(CmdError::Params("path must end in .docx".into()));
    }
    let path = crate::io::unused_output(requested).map_err(CmdError::Failed)?;
    let page_size = string(v, "page_size")?.unwrap_or("letter");
    let (page_w, page_h) = match page_size {
        "letter" => (612.0, 792.0),
        "a4" => (595.28, 841.89),
        _ => return Err(CmdError::Params("page_size is letter or a4".into())),
    };
    let margin = number(v, "margin", 60.0, 36.0, 108.0)?;
    let font = string(v, "font")?.unwrap_or("DejaVu Sans");
    if font.is_empty() || font.len() > 128 || font.chars().any(|c| c.is_control() || matches!(c, '\u{fffe}' | '\u{ffff}')) {
        return Err(CmdError::Params("font must be 1..128 bytes with no controls".into()));
    }
    let size = number(v, "font_size", 11.0, 9.0, 16.0)?;
    let page =
        SectionProps { page_w, page_h, margin_top: margin, margin_bottom: margin, margin_left: margin, margin_right: margin, ..Default::default() };
    let mut flow = wordcraft_formats::markdown::parse(markdown);
    if let Some(title) = title.filter(|t| !t.is_empty()) {
        flow.blocks.insert(
            0,
            FBlock::Para(Para {
                kind: Kind::Title,
                keep_next: true,
                inlines: vec![Inline::Text(title.into(), Fmt::default())],
                ..Default::default()
            }),
        );
        flow.meta.title = title.into();
    }
    let (mut blocks, mut cells) = (0usize, 0usize);
    limits(&flow.blocks, 0, &mut blocks, &mut cells)?;
    let mut doc = wordcraft_formats::model::to_doc_in(&flow, page);
    for block in &mut doc.body {
        if let wordcraft_doc::Block::Table(table) = Arc::make_mut(block) {
            table.props.style = Some("GridTable4AccentBlue".into());
            table.props.cell_margins = Some([4.0, 6.0, 4.0, 6.0]);
            table.props.look.first_column = false;
            table.props.look.header_row = true;
            table.props.look.banded_rows = true;
            table.props.space_after = Some(8.0);
        }
    }
    let styles = Arc::make_mut(&mut doc.styles);
    styles.default_chr.font = Some(font.into());
    styles.default_chr.size = Some(size);
    styles.default_para.space_after = Some(8.0);
    styles.default_para.line_spacing = Some(LineSpacing::Multiple(1.15));
    for style in &mut styles.styles {
        if style.kind == wordcraft_doc::styles::StyleKind::Paragraph && (style.id.starts_with("Heading") || style.id == "Title") {
            style.chr.font = Some(font.into());
            style.para.keep_next = Some(true);
            style.para.keep_lines = Some(true);
        }
    }
    let mut warnings = vec!["Supported Markdown: headings, paragraphs, lists, pipe tables, links, emphasis, code and quotes. Raw HTML, footnotes and extensions outside this subset are not interpreted.".to_string()];
    warnings.extend(wordcraft_formats::model::import_warnings());
    let bytes = crate::io::save_bytes("Document.docx", &doc).map_err(CmdError::Failed)?;
    if bytes.len() > crate::io::MAX_DELIVERY {
        return Err(CmdError::Params("DOCX exceeds the 32 MiB delivery limit; nothing was published".into()));
    }
    crate::io::write_new_file(&path.to_string_lossy(), &bytes).map_err(CmdError::Failed)?;
    let result = json!({"path": path, "bytes": bytes.len(), "paragraphs": doc.paragraph_count(), "words": doc.word_count(), "blocks": blocks, "table_cells": cells, "warnings": warnings});
    s.add_opened_document(doc);
    s.path = Some(path);
    s.dirty = false;
    Ok(result)
}

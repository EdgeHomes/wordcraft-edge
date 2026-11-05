//! Embedded files use the standard MCP resource content understood by LibreChat.
use serde_json::{Value, json};
use std::path::Path;
use wordcraft_engine::{
    file_access::{self, Access},
    io,
};

pub fn mime_of(path: &Path) -> Result<&'static str, String> {
    match path.extension().and_then(|s| s.to_str()).unwrap_or_default().to_ascii_lowercase().as_str() {
        "docx" => Ok("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        "pptx" => Ok("application/vnd.openxmlformats-officedocument.presentationml.presentation"),
        "xlsx" => Ok("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        "pdf" => Ok("application/pdf"),
        "csv" => Ok("text/csv"),
        _ => Err("file_attach supports docx, pptx, xlsx, pdf and csv".into()),
    }
}

pub fn file_content(path: &Path, data: &[u8]) -> Result<Value, String> {
    let mime = mime_of(path)?;
    let name = path.file_name().and_then(|s| s.to_str()).ok_or("filename must be Unicode")?;
    let mut encoded = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(b));
        } else {
            use std::fmt::Write;
            let _ = write!(encoded, "%{b:02X}");
        }
    }
    Ok(
        json!({"type":"resource","resource":{"uri":format!("file:///{encoded}"),"mimeType":mime,"blob":wordcraft_engine::cmd::insert::base64_encode(data)}}),
    )
}

pub fn attach(args: &Value) -> Result<Value, String> {
    let path = args.get("path").and_then(Value::as_str).ok_or("file_attach needs string path")?;
    let path = file_access::checked_path(path, Access::Read)?;
    mime_of(&path)?;
    let data = io::read_file_capped(&path.to_string_lossy(), io::MAX_DELIVERY as u64, "attachment")?;
    Ok(json!({"content":[
        {"type":"text", "text":json!({"path":path,"bytes":data.len(),"attached":true}).to_string()},
        file_content(&path, &data)?
    ]}))
}

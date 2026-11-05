//! JSON-RPC 2.0 framing and the MCP lifecycle / tools / resources methods.

use std::io::{BufRead, Read, Write};

use serde_json::{Value, json};

use crate::backend::Backend;
use crate::tools::{call_tool, tool_definitions};

/// The MCP revision we implement.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
const SUPPORTED_VERSIONS: &[&str] = &[PROTOCOL_VERSION, "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;
const RESOURCE_NOT_FOUND: i64 = -32002;

pub const WORKFLOW: &str = "WordCraft makes and edits documents. To make a new document, call doc_create. When the user attaches a document, its absolute path is provided in context: open that exact path with WordCraft, inspect and edit it, save under a new name, then call file_attach so the user can download it. Never write Python or other code to create, read or edit a document. To give the user any file a tool wrote, call file_attach with its path.";
const INSTRUCTIONS: &str = "Use inspect_document and get_text to inspect the opened structure. Select text and type_text to replace it; execute and batch reach formatting, tables, find/replace and review commands. list_commands discovers command parameters. Check import warnings: modeled formatting is preserved, but unsupported DOCX content may change or be lost. Rooted saves always publish a new file, preserving uploads. tool_search discovers other tools; tool_call returns their exact content, including files. Tools accept doc to address an open document without switching it.";

/// Resource URIs.
pub const DOC_URI: &str = "wordcraft://document";
pub const COMMANDS_URI: &str = "wordcraft://commands";

/// Longest request line accepted on stdin.
pub const MAX_REQUEST_LINE: usize = 1 << 20;

/// One bounded line read.
pub(crate) enum BoundedLine {
    Eof,
    Line(String),
    /// The line exceeded the limit; the rest of it has been discarded.
    TooLong,
}

/// Read one line of at most `max` bytes (excluding the newline) without buffering more than
/// `max + 1` bytes; an over-long line is skipped up to its newline.
pub(crate) fn read_bounded_line(reader: &mut impl BufRead, max: usize) -> std::io::Result<BoundedLine> {
    let mut buf = Vec::new();
    reader.by_ref().take(max as u64 + 1).read_until(b'\n', &mut buf)?;
    if buf.is_empty() {
        return Ok(BoundedLine::Eof);
    }
    if buf.last() != Some(&b'\n') && buf.len() > max {
        loop {
            let chunk = reader.fill_buf()?;
            if chunk.is_empty() {
                break;
            }
            let found = chunk.iter().position(|&b| b == b'\n');
            let used = found.map_or(chunk.len(), |i| i + 1);
            reader.consume(used);
            if found.is_some() {
                break;
            }
        }
        return Ok(BoundedLine::TooLong);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
    }
    String::from_utf8(buf).map(BoundedLine::Line).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// An MCP server bound to one backend.
pub struct Server {
    backend: Box<dyn Backend>,
    initialized: bool,
    compact: bool,
    roots: Option<std::sync::Arc<wordcraft_engine::file_access::AutomationRoots>>,
}

fn response(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

impl Server {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        Self { backend, initialized: false, compact: false, roots: None }
    }

    pub fn with_compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    pub fn with_root(mut self, root: &std::path::Path) -> Result<Self, String> {
        if self.backend.has_ui() {
            return Err("--root requires headless mode; remote confinement cannot be enforced".into());
        }
        self.roots = Some(wordcraft_engine::file_access::AutomationRoots::rooted(root)?);
        Ok(self)
    }

    pub fn backend(&mut self) -> &mut dyn Backend {
        self.backend.as_mut()
    }

    /// Whether the client has sent `notifications/initialized`.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Serve newline-delimited JSON-RPC until `input` closes. Logs go to stderr only (stdout is
    /// the protocol stream).
    pub fn serve(&mut self, mut input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
        loop {
            let reply = match read_bounded_line(&mut input, MAX_REQUEST_LINE)? {
                BoundedLine::Eof => break,
                BoundedLine::Line(line) => self.handle_line(&line),
                BoundedLine::TooLong => Some(error(Value::Null, INVALID_REQUEST, "request line too long").to_string()),
            };
            if let Some(reply) = reply {
                output.write_all(reply.as_bytes())?;
                output.write_all(b"\n")?;
                output.flush()?;
            }
        }
        Ok(())
    }

    /// Handle one line; returns the reply line (None for notifications and blank lines).
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(Value::Array(batch)) => {
                // Batches were removed in 2025-06-18; still answer older clients sensibly.
                if batch.is_empty() {
                    Some(error(Value::Null, INVALID_REQUEST, "empty batch"))
                } else {
                    let replies: Vec<Value> = batch.into_iter().filter_map(|m| self.handle(m)).collect();
                    (!replies.is_empty()).then_some(Value::Array(replies))
                }
            }
            Ok(msg) => self.handle(msg),
            Err(e) => Some(error(Value::Null, PARSE_ERROR, format!("parse error: {e}"))),
        };
        reply.map(|r| r.to_string())
    }

    /// Handle one JSON-RPC message; `None` for notifications and responses.
    pub fn handle(&mut self, msg: Value) -> Option<Value> {
        let Value::Object(o) = &msg else { return Some(error(Value::Null, INVALID_REQUEST, "message must be an object")) };
        let id = o.get("id").cloned();
        let Some(method) = o.get("method").and_then(Value::as_str) else {
            // A response to a server→client request (we send none): ignore. Anything else is invalid.
            if o.contains_key("result") || o.contains_key("error") {
                return None;
            }
            return Some(error(id.unwrap_or(Value::Null), INVALID_REQUEST, "missing `method`"));
        };
        let params = o.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = id else {
            self.notification(method);
            return None;
        };
        if !(id.is_string() || id.is_number()) {
            return Some(error(Value::Null, INVALID_REQUEST, "`id` must be a string or number"));
        }
        Some(match self.request(method, &params) {
            Ok(r) => response(id, r),
            Err((code, m)) => error(id, code, m),
        })
    }

    fn notification(&mut self, method: &str) {
        match method {
            "notifications/initialized" => self.initialized = true,
            "notifications/cancelled" | "notifications/progress" | "notifications/roots/list_changed" => {}
            other => log::debug!("ignoring notification {other}"),
        }
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        let roots = self.roots.clone();
        wordcraft_engine::file_access::confine(roots.as_ref(), || self.request_confined(method, params))
    }

    fn request_confined(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL_VERSION);
                let version = if SUPPORTED_VERSIONS.contains(&asked) { asked } else { PROTOCOL_VERSION };
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}, "resources": {}},
                    "serverInfo": {"name": "wordcraft", "title": "WordCraft", "version": env!("CARGO_PKG_VERSION"), "websiteUrl": "https://getartcraft.com/apps/wordcraft"},
                    "instructions": format!(
                        "{WORKFLOW} {INSTRUCTIONS} Backend: {}. Community: https://discord.gg/artcraft · https://getartcraft.com/apps/wordcraft · https://github.com/storytold/wordcraft",
                        self.backend.describe()
                    ),
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": if self.compact { Value::Array(crate::compact::definitions()) } else { tool_definitions() }})),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).ok_or((INVALID_PARAMS, "missing tool `name`".to_string()))?;
                let args = params.get("arguments").cloned().unwrap_or(Value::Null);
                let roots = self.roots.clone();
                Ok(wordcraft_engine::file_access::confine(roots.as_ref(), || call_tool(self.backend.as_mut(), name, &args).to_value()))
            }
            "resources/list" => Ok(json!({"resources": [
                {"uri": DOC_URI, "name": "document", "title": "Active document", "description": "Blocks, text, formatting, sections, parts and selection of the document (document.inspect)", "mimeType": "application/json"},
                {"uri": COMMANDS_URI, "name": "commands", "title": "Commands", "description": "Every command: id, label, ribbon location, shortcut, params, enabled (engine.commands)", "mimeType": "application/json"},
            ]})),
            "resources/templates/list" => Ok(json!({"resourceTemplates": []})),
            "resources/read" => {
                let uri = params.get("uri").and_then(Value::as_str).ok_or((INVALID_PARAMS, "missing `uri`".to_string()))?;
                let v = match uri {
                    DOC_URI => self.backend.call("document.inspect", json!({})),
                    COMMANDS_URI => self.backend.call("engine.commands", json!({})),
                    _ => return Err((RESOURCE_NOT_FOUND, format!("resource not found: {uri}"))),
                }
                .map_err(|e| (INTERNAL_ERROR, e))?;
                Ok(json!({"contents": [{"uri": uri, "mimeType": "application/json", "text": serde_json::to_string_pretty(&v).unwrap_or_default()}]}))
            }
            other => Err((METHOD_NOT_FOUND, format!("method not found: {other}"))),
        }
    }
}

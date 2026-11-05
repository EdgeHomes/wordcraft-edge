use serde_json::{Value, json};
use std::path::PathBuf;
use wordcraft_engine::{
    file_access::{self, AutomationRoots},
    io,
};
use wordcraft_mcp::{Headless, Server};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!("wc-delivery-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&p).unwrap();
        Self(p.canonicalize().unwrap())
    }
    fn server(&self) -> Server {
        Server::new(Box::new(Headless::default())).with_root(&self.0).unwrap().with_compact(true)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn rpc(s: &mut Server, method: &str, params: Value) -> Value {
    s.handle(json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})).unwrap()["result"].clone()
}
fn call(s: &mut Server, name: &str, args: Value) -> Value {
    rpc(s, "tools/call", json!({"name":name,"arguments":args}))
}
fn ok(s: &mut Server, name: &str, args: Value) -> Value {
    let r = call(s, name, args);
    assert_eq!(r["isError"], false, "{name}: {r}");
    r
}
fn summary(r: &Value) -> Value {
    serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap()
}
fn exec(s: &mut Server, command: &str, params: Value) -> Value {
    ok(s, "execute", json!({"command":command,"params":params}))
}
const MARKDOWN: &str = "# Executive summary\n\nA **professional** proposal with *careful* planning and a [reference](https://example.org).\n\n## Scope\n\n- Discovery\n- Delivery\n\n1. Research\n2. Review\n\n| Phase | Cost |\n| --- | ---: |\n| Design | $1200 |\n| Delivery | $3000 |\n\nFinal recommendation.";
fn create(s: &mut Server) -> Value {
    ok(s, "doc_create", json!({"title":"Project proposal","markdown":MARKDOWN,"path":"Proposal.docx","page_size":"a4"}))
}

#[test]
fn create_semantic_docx_reopen_and_attach() {
    let d = Scratch::new();
    let mut s = d.server();
    let r = create(&mut s);
    let info = summary(&r);
    assert!(info["words"].as_u64().unwrap() > 20);
    assert_eq!(info["table_cells"], 6);
    assert!(info["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("Raw HTML")));
    let path = info["path"].as_str().unwrap();
    let bytes = std::fs::read(path).unwrap();
    assert!(bytes.starts_with(b"PK"));
    let resource = &r["content"][1]["resource"];
    assert_eq!(resource["mimeType"], "application/vnd.openxmlformats-officedocument.wordprocessingml.document");
    assert_eq!(wordcraft_engine::cmd::insert::base64_decode(resource["blob"].as_str().unwrap()).unwrap(), bytes);
    let doc = io::open_path(std::path::Path::new(path)).unwrap();
    assert_eq!(doc.styles.default_chr.font.as_deref(), Some("DejaVu Sans"));
    assert_eq!(doc.styles.default_chr.size, Some(11.0));
    assert_eq!(doc.styles.get("Heading1").unwrap().para.keep_next, Some(true));
    assert!((doc.last_section.page_w - 595.28).abs() < 0.1);
    assert_eq!(doc.last_section.margin_left, 60.0);
    assert!(doc.numbering.abstracts.iter().flat_map(|n| &n.levels).any(|l| l.format == wordcraft_doc::section::NumFormat::Bullet));
    assert!(doc.numbering.abstracts.iter().flat_map(|n| &n.levels).any(|l| l.format == wordcraft_doc::section::NumFormat::Decimal));
    let table = doc.body.iter().find_map(|b| b.as_table()).unwrap();
    assert_eq!(table.props.style.as_deref(), Some("GridTable4AccentBlue"));
    assert_eq!(table.props.cell_margins, Some([4.0, 6.0, 4.0, 6.0]));
    assert_eq!(doc.styles.get("GridTable4AccentBlue").unwrap().table.as_ref().unwrap().header_chr.bold, Some(true));
    let mut reopened = d.server();
    ok(&mut reopened, "open_document", json!({"path":path}));
    let inspect = summary(&ok(&mut reopened, "inspect_document", json!({})));
    let blocks = inspect["blocks"].as_array().unwrap();
    assert_eq!(blocks[0]["style"], "Title");
    assert_eq!(blocks[1]["style"], "Heading1");
    assert!(blocks.iter().any(|b| b["type"] == "table"));
    assert!(blocks.iter().any(|b| !b["list"].is_null()));
    let runs: Vec<&Value> = blocks.iter().filter_map(|b| b["runs"].as_array()).flatten().collect();
    assert!(runs.iter().any(|r| r["props"]["bold"] == true));
    assert!(runs.iter().any(|r| r["props"]["italic"] == true));
    assert!(runs.iter().any(|r| r["props"]["link"] == "https://example.org"));
    let attached = ok(&mut reopened, "file_attach", json!({"path":path}));
    assert_eq!(r["content"][1], attached["content"][1]);
    let again = summary(&create(&mut s));
    assert_ne!(again["path"], info["path"]);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn absolute_upload_edit_save_as_preserves_bytes_and_formatting() {
    let d = Scratch::new();
    let mut s = d.server();
    let r = create(&mut s);
    let original = summary(&r)["path"].as_str().unwrap().to_string();
    let original_bytes = std::fs::read(&original).unwrap();
    let upload_dir = d.0.join("uploads/uuid");
    std::fs::create_dir_all(&upload_dir).unwrap();
    let upload = upload_dir.join("Original.docx");
    std::fs::write(&upload, &original_bytes).unwrap();
    let mut editing = d.server();
    let open = summary(&ok(&mut editing, "open_document", json!({"path":upload})));
    assert!(!open["warnings"].as_array().unwrap().is_empty());
    ok(&mut editing, "select_text", json!({"text":"careful"}));
    ok(&mut editing, "type_text", json!({"text":"precise"}));
    let saved = summary(&ok(&mut editing, "save_document", json!({})));
    assert_ne!(saved["path"], json!(upload));
    assert!(saved["path"].as_str().unwrap().ends_with(".docx"));
    let out = saved["path"].as_str().unwrap();
    let attach = ok(&mut editing, "file_attach", json!({"path":out}));
    assert_eq!(
        wordcraft_engine::cmd::insert::base64_decode(attach["content"][1]["resource"]["blob"].as_str().unwrap()).unwrap(),
        std::fs::read(out).unwrap()
    );
    let doc_before = io::open_bytes("original.docx", &original_bytes).unwrap();
    let doc_after = io::open_path(std::path::Path::new(out)).unwrap();
    assert_eq!(doc_before.styles, doc_after.styles);
    assert_eq!(doc_before.last_section, doc_after.last_section);
    assert_eq!(doc_before.body.first(), doc_after.body.first(), "unaffected title survives exactly");
    assert_eq!(doc_before.body.len(), doc_after.body.len());
    for (before, after) in doc_before.body.iter().zip(&doc_after.body) {
        if before.as_para().is_some_and(|p| p.text.contains("careful")) {
            continue;
        }
        assert_eq!(before, after, "every unaffected paragraph, list and table survives exactly");
    }
    let mut verify = d.server();
    ok(&mut verify, "open_document", json!({"path":out}));
    let text = summary(&ok(&mut verify, "get_text", json!({})));
    assert!(text["text"].as_str().unwrap().contains("precise"));
    assert!(!text["text"].as_str().unwrap().contains("careful"));
    assert_eq!(std::fs::read(&upload).unwrap(), original_bytes);
    // Explicitly asking for the original also chooses a new name.
    let saved = summary(&ok(&mut editing, "save_document", json!({"path":upload})));
    assert_ne!(saved["path"], json!(upload));
    assert_eq!(std::fs::read(&upload).unwrap(), original_bytes);
}

#[test]
fn compact_discovery_and_meta_file_equivalence() {
    let d = Scratch::new();
    let mut s = d.server();
    let init = rpc(&mut s, "initialize", json!({}));
    assert!(init["instructions"].as_str().unwrap().starts_with(wordcraft_mcp::server::WORKFLOW));
    let tools = rpc(&mut s, "tools/list", json!({}));
    let tools = tools["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 16);
    for name in wordcraft_mcp::compact::CORE {
        assert!(tools.iter().any(|t| t["name"] == *name));
    }
    assert!(!tools.iter().any(|t| t["name"].as_str().unwrap().contains("directory")));
    let schema = summary(&ok(&mut s, "tool_search", json!({"name":"doc_create"})));
    assert!(schema["inputSchema"]["properties"]["markdown"].is_object());
    let r = create(&mut s);
    let path = summary(&r)["path"].clone();
    let direct = ok(&mut s, "file_attach", json!({"path":path}));
    let meta = ok(&mut s, "tool_call", json!({"name":"file_attach","arguments":{"path":path}}));
    assert_eq!(direct, meta);
    let meta_create = ok(&mut s, "tool_call", json!({"name":"doc_create","arguments":{"markdown":"# Meta","path":"Meta.docx"}}));
    assert_eq!(meta_create["content"][1]["type"], "resource");
    for name in ["tool_search", "tool_call"] {
        assert_eq!(call(&mut s, "tool_call", json!({"name":name}))["isError"], true);
    }
}

#[test]
fn hostile_arguments_limits_and_malformed_documents() {
    let d = Scratch::new();
    let mut s = d.server();
    for a in [
        json!({}),
        json!({"markdown":"x", "title":"\u{fffe}"}),
        json!({"markdown":13}),
        json!({"markdown":""}),
        json!({"markdown":"x","page_size":"b0"}),
        json!({"markdown":"x","font_size":"11"}),
        json!({"markdown":"x","margin":500}),
        json!({"markdown":"x","unknown":true}),
        json!({"markdown":"x","path":"x.pdf"}),
        json!({"markdown":"![alt](data:image/png;base64,AAAA)"}),
        json!({"markdown":"x".repeat((512<<10)+1)}),
        json!({"markdown":"[".repeat(17)}),
        json!({"markdown":">".repeat(17)}),
        json!({"markdown":format!("{}x", " ".repeat(65))}),
    ] {
        assert_eq!(call(&mut s, "doc_create", a.clone())["isError"], true, "{a}");
    }
    let table = format!("|a|b|\n|---|---|\n{}", "|x|y|\n".repeat(200));
    assert_eq!(call(&mut s, "doc_create", json!({"markdown":table}))["isError"], true);
    let wide = format!("|{}\n|{}", "x|".repeat(17), "---|".repeat(17));
    assert_eq!(call(&mut s, "doc_create", json!({"markdown":wide}))["isError"], true);
    assert_eq!(std::fs::read_dir(&d.0).unwrap().count(), 0, "invalid creation publishes nothing");
    std::fs::write(d.0.join("bad.docx"), b"PK broken zip").unwrap();
    assert_eq!(call(&mut s, "open_document", json!({"path":"bad.docx"}))["isError"], true);
    assert_eq!(call(&mut s, "file_attach", json!({"path":"."}))["isError"], true);
    std::fs::write(d.0.join("bad.exe"), b"x").unwrap();
    assert_eq!(call(&mut s, "file_attach", json!({"path":"bad.exe"}))["isError"], true);
    let huge = std::fs::File::create(d.0.join("huge.docx")).unwrap();
    huge.set_len((32 << 20) + 1).unwrap();
    assert_eq!(call(&mut s, "file_attach", json!({"path":"huge.docx"}))["isError"], true);
}

#[test]
fn mime_types_and_percent_encoded_filenames() {
    let d = Scratch::new();
    let mut s = d.server();
    for (ext, mime) in [
        ("docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        ("pptx", "application/vnd.openxmlformats-officedocument.presentationml.presentation"),
        ("xlsx", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        ("pdf", "application/pdf"),
        ("csv", "text/csv"),
    ] {
        let name = format!("résumé #?%.{ext}");
        std::fs::write(d.0.join(&name), b"abc").unwrap();
        let r = ok(&mut s, "file_attach", json!({"path":name}));
        assert_eq!(r["content"][1]["resource"]["mimeType"], mime);
        assert_eq!(r["content"][1]["resource"]["uri"], format!("file:///r%C3%A9sum%C3%A9%20%23%3F%25.{ext}"));
    }
}

#[test]
fn confinement_generic_batch_nested_links_and_templates() {
    let d = Scratch::new();
    let outside = Scratch::new();
    let mut s = d.server();
    std::fs::write(outside.0.join("secret.md"), "secret").unwrap();
    let secret = outside.0.join("secret.md");
    for path in [secret.to_string_lossy().into_owned(), "../secret.md".into()] {
        assert_eq!(call(&mut s, "open_document", json!({"path":path}))["isError"], true);
        assert_eq!(call(&mut s, "execute", json!({"command":"file.open","params":{"path":path}}))["isError"], true);
        assert_eq!(call(&mut s, "batch", json!({"commands":[{"command":"file.open","params":{"path":path}}]}))["isError"], true);
        assert_eq!(call(&mut s, "tool_call", json!({"name":"execute","arguments":{"command":"file.open","params":{"path":path}}}))["isError"], true);
        exec(&mut s, "tools.macros", json!({"define":{"name":"escape","steps":[{"command":"file.open","params":{"path":path}}]}}));
        assert_eq!(call(&mut s, "execute", json!({"command":"tools.macros","params":{"run":"escape"}}))["isError"], true);
        for command in ["insert.picture", "file.newFromTemplate", "insert.textFromFile"] {
            assert_eq!(call(&mut s, "execute", json!({"command":command,"params":{"path":path}}))["isError"], true);
        }
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside.0, d.0.join("away")).unwrap();
        assert_eq!(call(&mut s, "open_document", json!({"path":"away/secret.md"}))["isError"], true);
        assert_eq!(call(&mut s, "doc_create", json!({"markdown":"x","path":"away/leak.docx"}))["isError"], true);
    }
    std::fs::write(d.0.join("inside.md"), "# Inside").unwrap();
    exec(&mut s, "file.open", json!({"path":"inside.md"}));
    let r = summary(&exec(&mut s, "file.save", json!({"path":"out.docx"})));
    assert!(d.0.join("out.docx").is_file());
    assert_eq!(r["path"], json!(d.0.join("out.docx")));
    let roots = AutomationRoots::rooted(&d.0).unwrap();
    file_access::confine(Some(&roots), || {
        assert!(io::read_file(&secret.to_string_lossy()).is_err());
        assert!(io::read_regular_file(&secret, 10).is_none());
        assert!(io::write_file("../escape.docx", b"bad").is_err());
    });
}

#[test]
fn atomic_non_overwrite_and_output_cap() {
    let d = Scratch::new();
    let roots = AutomationRoots::rooted(&d.0).unwrap();
    file_access::confine(Some(&roots), || {
        io::write_new_file("existing.docx", b"original").unwrap();
        assert!(io::write_new_file("existing.docx", b"replacement").is_err());
        assert!(io::write_file("existing.docx", b"replacement").is_err());
        assert_eq!(std::fs::read(d.0.join("existing.docx")).unwrap(), b"original");
        assert!(io::write_new_file("huge.docx", &vec![0; (32 << 20) + 1]).is_err());
        assert!(!d.0.join("huge.docx").exists());
        // Simulate another writer claiming the chosen name before publication.
        let chosen = io::unused_output("race.docx").unwrap();
        std::fs::write(&chosen, b"winner").unwrap();
        assert!(io::write_new_file(&chosen.to_string_lossy(), b"loser").is_err());
        assert_eq!(std::fs::read(chosen).unwrap(), b"winner");
    });
    assert!(!file_access::rooted(), "authority restored after request");
    assert_eq!(std::fs::read_dir(&d.0).unwrap().count(), 2, "staging files cleaned");
}

#[test]
fn rooted_html_linked_media_stays_inside_and_remote_is_refused() {
    let d = Scratch::new();
    let out = Scratch::new();
    let mut s = d.server();
    let png =
        wordcraft_engine::cmd::insert::base64_decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j2mQAAAAASUVORK5CYII=")
            .unwrap();
    std::fs::write(out.0.join("outside.png"), &png).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(out.0.join("outside.png"), d.0.join("leak.png")).unwrap();
    std::fs::write(d.0.join("linked.html"), "<p>Inside<img src=\"leak.png\"></p>").unwrap();
    let roots = AutomationRoots::rooted(&d.0).unwrap();
    file_access::confine(Some(&roots), || {
        let doc = io::open_path(&d.0.join("linked.html")).unwrap();
        assert!(doc.media.is_empty(), "symlinked outside picture is not imported");
    });
    std::fs::write(d.0.join("inside.png"), &png).unwrap();
    std::fs::write(d.0.join("linked.html"), "<p>Inside<img src=\"inside.png\"></p>").unwrap();
    file_access::confine(Some(&roots), || {
        let doc = io::open_path(std::path::Path::new("linked.html")).unwrap();
        assert!(!doc.media.is_empty(), "relative HTML picture inside root is imported");
    });
    ok(&mut s, "open_document", json!({"path":"linked.html"}));
    // A remote backend is refused before it connects.
    let remote = wordcraft_mcp::Remote::lazy("127.0.0.1:7981");
    assert!(Server::new(Box::new(remote)).with_root(&d.0).is_err());
}

#[test]
fn traversal_inside_root_is_rejected_too() {
    let d = Scratch::new();
    let mut s = d.server();
    std::fs::create_dir(d.0.join("sub")).unwrap();
    std::fs::write(d.0.join("inside.md"), "inside").unwrap();
    assert_eq!(call(&mut s, "open_document", json!({"path":"sub/../inside.md"}))["isError"], true);
    assert_eq!(call(&mut s, "doc_create", json!({"markdown":"x","title":"\u{fffe}"}))["isError"], true);
}

#[test]
fn multiline_inline_nesting_is_bounded() {
    let d = Scratch::new();
    let mut s = d.server();
    assert_eq!(call(&mut s, "doc_create", json!({"markdown":"[\n".repeat(17)}))["isError"], true);
    assert_eq!(std::fs::read_dir(&d.0).unwrap().count(), 0);
}

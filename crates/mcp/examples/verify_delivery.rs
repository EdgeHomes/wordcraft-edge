//! Stdio acceptance harness; drives native CLI, npm launcher, or Docker Alpine transparently.
//! cargo run -p wordcraft-mcp --example verify_delivery -- HOST_ROOT SERVER_ROOT COMMAND [ARG...]
//! The harness appends --compact --root SERVER_ROOT. For CLI, include `mcp` in ARG.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use serde_json::{Value, json};
use std::{
    error::Error,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
fn check(ok: bool, why: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(why.into()) }
}
struct Client {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}
impl Client {
    fn new(command: &[String], root: &str) -> Result<Self> {
        let (exe, args) = command.split_first().ok_or("missing command")?;
        let mut child = Command::new(exe)
            .args(args)
            .args(["--compact", "--root", root])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let input = child.stdin.take().ok_or("no stdin")?;
        let output = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        Ok(Self { child, input: Some(input), output })
    }
    fn rpc(&mut self, method: &str, params: Value) -> Result<Value> {
        let input = self.input.as_mut().ok_or("client closed")?;
        writeln!(input, "{}", json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))?;
        input.flush()?;
        let mut line = String::new();
        self.output.read_line(&mut line)?;
        let v: Value = serde_json::from_str(&line)?;
        check(v.get("error").is_none(), &format!("RPC error: {v}"))?;
        v.get("result").cloned().ok_or_else(|| "no result".into())
    }
    fn call(&mut self, name: &str, args: Value) -> Result<Value> {
        let v = self.rpc("tools/call", json!({"name":name,"arguments":args}))?;
        check(v["isError"] == false, &format!("{name}: {v}"))?;
        Ok(v)
    }
    fn exec(&mut self, name: &str, args: Value) -> Result<Value> {
        self.call("execute", json!({"command":name,"params":args}))
    }
    fn close(mut self) -> Result<()> {
        drop(self.input.take());
        check(self.child.wait()?.success(), "server exited unsuccessfully")
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        drop(self.input.take());
        let _ = self.child.wait();
    }
}
fn summary(v: &Value) -> Result<Value> {
    Ok(serde_json::from_str(v["content"][0]["text"].as_str().ok_or("missing summary")?)?)
}
fn embedded(v: &Value) -> Result<Vec<u8>> {
    let r = &v["content"][1]["resource"];
    check(r["mimeType"] == "application/vnd.openxmlformats-officedocument.wordprocessingml.document", "wrong DOCX MIME")?;
    wordcraft_engine::cmd::insert::base64_decode(r["blob"].as_str().ok_or("no blob")?).ok_or_else(|| "invalid base64".into())
}
fn map_path(host: &Path, server: &str, path: &str) -> Result<PathBuf> {
    Ok(host.join(Path::new(path).strip_prefix(server)?))
}
fn render(client: &mut Client, host: &Path, prefix: &str) -> Result<usize> {
    let mut pages = 1usize;
    for page in 1..=12 {
        let r = client.call("render_page", json!({"page":page,"scale":1}))?;
        let info: Value = serde_json::from_str(r["content"][1]["text"].as_str().ok_or("no render info")?)?;
        pages = usize::try_from(info["pages"].as_u64().ok_or("no pages")?)?;
        let bytes = wordcraft_engine::cmd::insert::base64_decode(r["content"][0]["data"].as_str().ok_or("no PNG")?).ok_or("bad PNG")?;
        std::fs::write(host.join(format!("{prefix}-{page}.png")), bytes)?;
        if page >= pages {
            break;
        }
    }
    check(pages <= 12, "fixture unexpectedly exceeds 12 pages")?;
    Ok(pages)
}
fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let host = PathBuf::from(args.first().ok_or("HOST_ROOT required")?);
    std::fs::create_dir_all(&host)?;
    let host = host.canonicalize()?;
    let server = args.get(1).ok_or("SERVER_ROOT required")?;
    let command = args.get(2..).ok_or("COMMAND required")?;
    let mut client = Client::new(command, server)?;
    let init = client.rpc("initialize", json!({"protocolVersion":"2025-06-18"}))?;
    check(init["instructions"].as_str().is_some_and(|s| s.starts_with(wordcraft_mcp::server::WORKFLOW)), "missing workflow steer")?;
    let tools = client.rpc("tools/list", json!({}))?;
    check(tools["tools"].as_array().is_some_and(|t| t.len() == 16), "wrong compact count")?;
    let mut markdown =
        String::from("# Executive summary\n\nA **professional** proposal with *careful* planning and a [reference](https://example.org).\n\n");
    for i in 1..=18 {
        markdown.push_str(&format!("## Phase {i}: Implementation\n\nThis phase delivers a clear plan, measurable milestones and a review with the project team. We will document decisions, resolve open questions and validate the outcome before moving to the next phase.\n\n- Confirm requirements and responsibilities\n- Review delivery and quality\n\n| Activity | Owner | Cost |\n| --- | --- | ---: |\n| Discovery | Project team | $1200 |\n| Delivery | Design team | $3000 |\n\n"));
    }
    let created = client
        .call("doc_create", json!({"title":"Professional project proposal","markdown":markdown,"path":"Proposal.docx","page_size":"letter"}))?;
    let info = summary(&created)?;
    let path = info["path"].as_str().ok_or("no create path")?;
    let original = embedded(&created)?;
    check(original == std::fs::read(map_path(&host, server, path)?)?, "creation bytes differ from resource")?;
    let doc = wordcraft_engine::io::open_bytes("Proposal.docx", &original)?;
    check(doc.styles.get("Heading1").is_some_and(|s| s.para.keep_next == Some(true)), "missing heading keeps")?;
    // Inspect the bytes the user receives, through a fresh session's DOCX reader.
    client.close()?;
    let mut client = Client::new(command, server)?;
    client.call("open_document", json!({"path":path}))?;
    let before = summary(&client.call("inspect_document", json!({}))?)?;
    let created_pages = render(&mut client, &host, "created")?;
    client.close()?;
    let upload_dir = host.join("uploads/synthetic-uuid");
    std::fs::create_dir_all(&upload_dir)?;
    let upload = upload_dir.join("Original.docx");
    std::fs::write(&upload, &original)?;
    let remote_upload = Path::new(server).join("uploads/synthetic-uuid/Original.docx");
    let mut client = Client::new(command, server)?;
    let opened = summary(&client.call("open_document", json!({"path":remote_upload}))?)?;
    check(opened["warnings"].is_array(), "missing import warnings")?;
    client.call("select_text", json!({"text":"careful"}))?;
    client.call("type_text", json!({"text":"precise"}))?;
    // Also prove formatting and table/review commands are discoverable in compact mode.
    let commands = summary(&client.call("list_commands", json!({"query":"table"}))?)?;
    check(commands.as_array().is_some_and(|c| !c.is_empty()), "no table commands")?;
    client.exec("select.text", json!({"text":"professional"}))?;
    client.exec("format.bold", json!({"value":true}))?;
    let saved = summary(&client.call("save_document", json!({"path":"Edited.docx"}))?)?;
    let saved_path = saved["path"].as_str().ok_or("missing save path")?;
    let attached = client.call("file_attach", json!({"path":saved_path}))?;
    check(attached == client.call("tool_call", json!({"name":"file_attach","arguments":{"path":saved_path}}))?, "direct/meta resource differs")?;
    let edited = embedded(&attached)?;
    check(edited == std::fs::read(map_path(&host, server, saved_path)?)?, "edited attachment differs")?;
    let edited_pages = render(&mut client, &host, "edited")?;
    client.close()?;
    check(std::fs::read(&upload)? == original, "upload was modified")?;
    let mut client = Client::new(command, server)?;
    client.call("open_document", json!({"path":saved_path}))?;
    let text = summary(&client.call("get_text", json!({}))?)?;
    check(text["text"].as_str().is_some_and(|s| s.contains("precise") && !s.contains("careful")), "targeted edit lost")?;
    let after = summary(&client.call("inspect_document", json!({}))?)?;
    check(before["blocks"][0] == after["blocks"][0], "unaffected title changed")?;
    let reopened = wordcraft_engine::io::open_bytes("Edited.docx", &edited)?;
    check(doc.styles == reopened.styles && doc.last_section == reopened.last_section, "unaffected formatting changed")?;
    client.close()?;
    println!(
        "{}",
        json!({"verified":true,"created_pages":created_pages,"edited_pages":edited_pages,"created_bytes":original.len(),"edited_bytes":edited.len(),"host_root":host})
    );
    Ok(())
}

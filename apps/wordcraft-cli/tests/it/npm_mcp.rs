use serde_json::{Value, json};
use std::io::Write;
use std::process::{Command, Stdio};
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wordcraft-cli"))
}
#[test]
fn tools_catalog_has_compact_download_workflows() {
    let o = cli().args(["tools", "--json", "--compact"]).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let tools: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(tools.as_array().unwrap().len(), 16);
    for name in ["doc_create", "file_attach", "open_document", "save_document", "execute", "list_commands", "tool_call"] {
        assert!(tools.as_array().unwrap().iter().any(|t| t["name"] == name), "{name}");
    }
}
#[test]
fn root_missing_remote_conflicts_and_relative_paths() {
    let dir = std::env::temp_dir().join(format!("wc-cli-root-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let absent = dir.join("missing").to_string_lossy().into_owned();
    let root = dir.to_string_lossy().into_owned();
    for args in [
        vec!["mcp", "--root", &absent],
        vec!["mcp", "--root", &root, "--connect", "127.0.0.1:7981"],
        vec!["mcp", "--root"],
        vec!["mcp", "--root="],
        vec!["mcp", "--root", &root, "--automation-read-root", &root],
        vec!["mcp", "--root", &root, "--root", &root],
    ] {
        let o = cli().args(&args).stdin(Stdio::null()).output().unwrap();
        assert!(!o.status.success(), "{args:?}");
        assert!(!o.stderr.is_empty());
    }
    for spelling in [vec!["--root".to_string(), root.clone()], vec![format!("--root={root}")]] {
        let mut child =
            cli().arg("mcp").arg("--compact").args(spelling).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"doc_create","arguments":{"markdown":"# CLI proposal","path":"result.docx"}}});
        writeln!(child.stdin.take().unwrap(), "{request}").unwrap();
        let o = child.wait_with_output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let r: Value = serde_json::from_slice(&o.stdout).unwrap();
        assert_eq!(r["result"]["isError"], false, "{r}");
        assert_eq!(r["result"]["content"][1]["type"], "resource");
    }
    assert!(dir.join("result.docx").is_file());
    assert!(dir.join("result (2).docx").is_file());
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn npm_launcher_roots_exit_status_and_signals() {
    use std::io::{BufRead, BufReader};
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("wc-launcher-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let stub = dir.join("stub");
    std::fs::write(&stub, "#!/bin/sh\nprintf '%s\\n' \"$@\"\nexit 37\n").unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let launcher = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/npm/wordcraft-edge-mcp/bin/wordcraft-edge-mcp.js");
    for (args, env_root, expected) in [
        (vec![], Some("from-env"), "from-env".to_string()),
        (vec![], None, dir.to_string_lossy().into_owned()),
        (vec!["--root", "explicit"], Some("ignored"), "explicit".into()),
        (vec!["--root=inline"], Some("ignored"), "inline".into()),
    ] {
        let mut c = Command::new("node");
        c.arg(&launcher).args(&args).env("WORDCRAFT_CLI", &stub).env_remove("WORDCRAFT_ROOT").current_dir(&dir);
        if let Some(root) = env_root {
            c.env("WORDCRAFT_ROOT", root);
        }
        let o = c.output().unwrap();
        assert_eq!(o.status.code(), Some(37));
        let stdout = String::from_utf8(o.stdout).unwrap();
        assert!(stdout.starts_with("mcp\n"));
        if args == ["--root=inline"] {
            assert_eq!(stdout, "mcp\n--root=inline\n");
        } else {
            assert!(stdout.ends_with(&format!("--root\n{expected}\n")), "{stdout}");
        }
    }
    std::fs::write(&stub, "#!/bin/sh\ntrap 'exit 23' TERM\necho ready\nwhile :; do sleep 0.1; done\n").unwrap();
    let mut child = Command::new("node").arg(&launcher).env("WORDCRAFT_CLI", &stub).stdout(Stdio::piped()).spawn().unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
    assert_eq!(line, "ready\n");
    assert!(Command::new("/bin/kill").args(["-TERM", &child.id().to_string()]).status().unwrap().success());
    assert_eq!(child.wait().unwrap().code(), Some(23));
    std::fs::remove_dir_all(dir).unwrap();
}

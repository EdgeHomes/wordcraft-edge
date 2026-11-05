# wordcraft-edge-mcp

WordCraft makes and edits documents through a native stdio MCP server. `doc_create` takes Markdown
and returns a styled, editable DOCX plus downloadable resource content in one call. For an upload,
open its exact absolute path, inspect and edit it, save under a new name, and call `file_attach`.
The model should never write code to create, read or edit the document.

```sh
npx -y wordcraft-edge-mcp --compact --root /existing/document-folder
```

The launcher resolves `wordcraft-edge-mcp-${process.platform}-${process.arch}`, or the executable
in `WORDCRAFT_CLI`, and runs `wordcraft-cli mcp ...args` with inherited stdio. Unless explicitly
supplied as `--root DIR` or `--root=DIR`, the root is `WORDCRAFT_ROOT` or the working directory.
It forwards SIGINT/SIGTERM/SIGHUP and child exit codes. Node 18 or newer is required.

## LibreChat

Add this beneath `mcpServers`:

```yaml
wordcraft:
  type: stdio
  command: npx
  args: ["-y", "wordcraft-edge-mcp", "--compact", "--root", "/app/uploads/wordcraft"]
  fileInputDir: /app/uploads/wordcraft
  serverInstructions: true
  timeout: 120000
```

**Create `/app/uploads/wordcraft` on the uploads volume before connecting.** Run on the node hosting LibreChat:

```sh
lc_container=$(docker ps --filter label=com.docker.swarm.service.name=librechat-test_api --format '{{.ID}}' | head -n 1)
docker exec "$lc_container" mkdir -p /app/uploads/wordcraft
```

No LibreChat source changes are needed for the EdgeHomes fork's DOCX resource downloads and
absolute upload paths. A shared root is suitable for development. Production requires per-user
roots and upload permissions; a root alone does not separate users sharing that directory.

## Tools and document fidelity

Compact mode exposes 14 core tools and two meta tools:
`doc_create`, `file_attach`, `open_document`, `save_document`, `inspect_document`, `get_text`,
`select_text`, `type_text`, `execute`, `list_commands`, `batch`, `new_document`, `list_documents`,
`render_page`, `tool_search`, `tool_call`. It has no directory-listing tool. Both create and edit
workflows need no discovery calls. `tool_call` returns exactly the direct tool's content,
including embedded files; recursive meta calls are refused.

```json
{"name":"doc_create","arguments":{"title":"Project proposal","markdown":"# Summary\n\nA **professional** proposal.\n\n## Scope\n\n- Discovery\n- Delivery","path":"Proposal.docx","page_size":"letter"}}
```

Creation uses WordCraft's existing Markdown parser and document model: real title/heading styles,
paragraphs, bullets/numbering, pipe tables, links and emphasis. Defaults are 11-point DejaVu Sans,
1.15 line spacing, 8-point paragraph spacing and 60-point margins. Heading styles keep with the
following paragraph; tables use bold shaded headers, row banding and padded cells. Supported settings: `page_size` (`letter`, `a4`), `font`, `font_size` (9–16),
and `margin` (36–108 points). Fonts are names, resolved through WordCraft's installed fonts or
open substitutes; no proprietary fonts are bundled.

Creation limits: 512 KiB Markdown, 4000 source lines/blocks, 8192 bytes per line, nesting 16,
tables up to 200 rows and 16 columns, and 4000 cells total. Markdown images are rejected (limit 0);
use the existing `insert.picture` command under the root after creation. Raw HTML, footnotes and
extensions outside the documented Markdown subset are not interpreted; creation returns a warning,
plus any importer limit reports. The generated result must fit 32 MiB before it is published.

Use `select_text` and `type_text` for targeted edits. `execute`/`batch` also reach find/replace,
paragraph/run formatting, tables and review commands; `list_commands` returns their actual params.
Uploaded documents are imported directly into the model, never rebuilt from extracted plain text.
Inspect the `open_document` warning: DOCX round trips preserve modeled structure/formatting, but
are not a guarantee of lossless arbitrary OOXML preservation. Unsupported features can change
or be lost; verify the saved document and see [file format parity](https://github.com/mbarker-edgehomes/wordcraft-edge/blob/main/docs/file-format-parity.md).
The upstream importer currently exposes log warnings rather than a structured per-feature loss
report; the open-tool warning is therefore conservative rather than an exhaustive inventory.

`file_attach` supports DOCX, PPTX, XLSX, PDF and CSV regular files up to 32 MiB. It checks metadata
and bounds the actual read. Each file is a standard embedded resource with a percent-encoded
filename URI, MIME type and base64 blob. DOCX is WordCraft's primary editable output.

## Filesystem behavior

`--root` must already exist. Relative paths resolve against it; absolute upload paths inside it
are accepted. Traversal and symlink escapes are rejected at existing engine read/write boundaries,
including generic/batch/macro dispatch, linked HTML media, pictures and templates. Rooted remote
`--connect` is refused, as remote I/O cannot be confined locally. The older separate read/write
root flags remain supported but cannot be combined with `--root`.

Rooted default saves choose a new `.docx` name. Explicit output paths also select an unused name;
all rooted writes publish fully prepared bytes without replacing an existing directory entry.
If another writer claims a selected name first, publication fails safely; retry with a new name.
Filesystems must support sibling hard links. Completed data is synced before publication; this
is not a promise of parent-directory durability after power loss. As upstream documents, confinement
checks paths before opening: it does not defend against a separate malicious local process racing
to replace parent directories. Isolate production directories and their filesystem permissions.

Diagnostic commands on the native executable:

```sh
wordcraft-cli tools
wordcraft-cli tools --json --compact
```

## Build, verify, publish

From the checkout, use Rust 1.97.1, Cargo Zigbuild and Zig:

```sh
RUSTUP_TOOLCHAIN=1.97.1 packaging/npm/stage.sh
```

Review [the verification record](../VERIFICATION.md) before publishing. Publishing is manual:

```sh
cd packaging/npm/dist && for d in wordcraft-edge-mcp-*/ wordcraft-edge-mcp/; do npm publish "$d" --access public || break; done
```

This publishes platform packages before the launcher and **stops on the first failure**. Handle
`npm login` and browser 2FA yourself. Expired login or approval timeout can appear as E404.
Do not infer success from the publish attempt. Verify all four registry versions:

```sh
for name in wordcraft-edge-mcp wordcraft-edge-mcp-darwin-arm64 wordcraft-edge-mcp-linux-x64 wordcraft-edge-mcp-linux-arm64; do
  npm view "$name" version --prefer-online || break
done
```

After verified publication and with deployment authorization, force-update `librechat-test_api`,
then check the installed npm version and `wordcraft-cli tools` in the running container. Test two
actual chat prompts: create a professional proposal and edit a specific DOCX upload. Confirm
both downloads, the original upload's bytes and the edited document's unaffected formatting.
Local binary/MCP checks do not establish live LibreChat acceptance.

If modelSpecs change separately, run `node config/litellm-tiers.js && node config/flush-cache.js`.

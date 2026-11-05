# npm MCP release verification

Date: **2026-11-05**. Staged version: **0.15.1**. Toolchain: **Rust 1.97.1**.
This records local verification. No npm publication or live LibreChat acceptance is claimed.

## Scoped quality gates

Builds and tests ran under explicit timeouts. The task explicitly requested affected-crate
scope, so the equivalent applicable gates were run rather than the GUI/workspace-wide
`cargo xtask ci`. CI files were not changed.

```sh
RUSTUP_TOOLCHAIN=1.97.1 cargo fmt --all -- --check
CRAFT_I18N_DIR=/tmp/wordcraft-i18n RUSTUP_TOOLCHAIN=1.97.1 cargo test --offline -p wordcraft-engine -p wordcraft-formats -p wordcraft-mcp -p wordcraft-cli
CRAFT_I18N_DIR=/tmp/wordcraft-i18n RUSTUP_TOOLCHAIN=1.97.1 cargo clippy --offline -p wordcraft-engine -p wordcraft-formats -p wordcraft-mcp -p wordcraft-cli --all-targets -- -D warnings
RUSTUP_TOOLCHAIN=1.97.1 cargo check --offline -p wordcraft-engine -p wordcraft-formats -p wordcraft-mcp --target wasm32-unknown-unknown
RUSTUP_TOOLCHAIN=1.97.1 cargo xtask assets
RUSTUP_TOOLCHAIN=1.97.1 cargo xtask layers
```

Results: **2,046 tests passed, 0 failed**, with one existing measurement test ignored:
`recovery_format::native_json_size_with_a_large_picture`. Breakdown: 32 CLI unit, 14 CLI
integration, 1,226 engine unit, 69 engine integration, 664 formats unit, 25 MCP unit and 16 MCP
integration tests. Engine `hostile_params_never_panic` passed. After strengthening the upload
preservation assertion, all 10 delivery workflow tests were rerun successfully; they compare
every unaffected paragraph, list and table, as well as styles and section properties. After the
final acceptance-harness edit, MCP clippy with all targets passed again. Format, clippy,
WASM, all 48 attributed assets and layering (19 crates, no violations) passed.

The first run without Language data exposed an existing German hyphenation assertion. The
complete passing run used craft-i18n at the repository's pin,
`9d6d54d6d9ff603cc41e12e5f6ba51af3554ab66`, checked out under `/tmp/wordcraft-i18n`.
Sandboxed runs could not bind the existing TCP control-channel tests; the passing run had
loopback permissions. Existing Cargo notices about unused egui patches remain. No language
packs, fonts, vendor documents or binary fixtures were added to the repository.

Logs on the verification machine: `/tmp/wordcraft-final-tests.log`,
`/tmp/wordcraft-workflows.log`, `/tmp/wordcraft-final-clippy.log`,
`/tmp/wordcraft-final-mcp-clippy.log`, `/tmp/wordcraft-wasm.log`,
`/tmp/wordcraft-assets.log`, `/tmp/wordcraft-layers.log` and `/tmp/wordcraft-fmt.log`.

## Regression proofs actually exercised

For each row below, the corresponding production behavior was temporarily removed. The named
regression produced **0 passed, 1 failed**, with a runtime assertion failure. Source was restored
in a `finally` block and the same test produced **1 passed**. Compilation failures did not count.
All **17** proofs completed. For rooted save-as, both the name selection and non-replacing writer
were reverted together to reproduce the former in-place save behavior. The actual-read test
captures metadata, grows the file, then confirms the read still stops at cap + 1.

| Behavior temporarily removed | Regression test |
|---|---|
| `create_dispatch` | `create_semantic_docx_reopen_and_attach` |
| `table_formatting` | `create_semantic_docx_reopen_and_attach` |
| `body_typography` | `create_semantic_docx_reopen_and_attach` |
| `rooted_save_as` | `absolute_upload_edit_save_as_preserves_bytes_and_formatting` |
| `compact_catalog` | `compact_discovery_and_meta_file_equivalence` |
| `meta_resources` | `compact_discovery_and_meta_file_equivalence` |
| `io_confinement` | `confinement_generic_batch_nested_links_and_templates` |
| `traversal` | `traversal_inside_root_is_rejected_too` |
| `image_limits` | `hostile_arguments_limits_and_malformed_documents` |
| `uri_encoding` | `mime_types_and_percent_encoded_filenames` |
| `attachment_cap` | `hostile_arguments_limits_and_malformed_documents` |
| `atomic_publication` | `atomic_non_overwrite_and_output_cap` |
| `output_cap` | `atomic_non_overwrite_and_output_cap` |
| `bounded_actual_read` | `io::delivery_tests::actual_read_is_bounded_when_file_grows_after_metadata` |
| `cli_root` | `npm_mcp::root_missing_remote_conflicts_and_relative_paths` |
| `cli_tools` | `npm_mcp::tools_catalog_has_compact_download_workflows` |
| `multiline_nesting` | `multiline_inline_nesting_is_bounded` |

Logs: `/tmp/wordcraft-red-<behavior>.log`, `/tmp/wordcraft-green-<behavior>.log`,
`/tmp/wordcraft-regression-proof.log`, and `/tmp/wordcraft-regression-results.json`.
These temporary logs and mutation scripts are not shipped.

## Staging, packing and native runtimes

`RUSTUP_TOOLCHAIN=1.97.1 packaging/npm/stage.sh` built and staged the final sources:

| Package | Target | Runtime checked |
|---|---|---|
| `wordcraft-edge-mcp-darwin-arm64` | `aarch64-apple-darwin` | Native macOS ARM64 |
| `wordcraft-edge-mcp-linux-x64` | `x86_64-unknown-linux-musl` | Disposable vanilla Alpine 3.22, Docker `linux/amd64` |
| `wordcraft-edge-mcp-linux-arm64` | `aarch64-unknown-linux-musl` | Disposable vanilla Alpine 3.22, Docker `linux/arm64` |
| `wordcraft-edge-mcp` | Node launcher | Fresh local offline npm installation with its Darwin optional dependency |

`file` identifies both Linux executables as **statically linked ELF**. No GNU Linux builds were
used. The Linux dependency audit found no egui/eframe/rfd/X11/Wayland dependency in the CLI's
normal graph. The shared font catalog does include Fontique's optional Fontconfig dlopen path;
creation, editing, attachment, inspection and rendering all ran successfully in vanilla Alpine
without adding Fontconfig or any desktop session. This does not qualify every language/font
catalog operation.

All four packages passed `npm pack`. Platform archives contain exactly `package.json` and
`bin/wordcraft-cli`; the launcher archive contains its JS entry point, metadata, README and two
license files. All optional platform dependencies are exactly `0.15.1`. A fresh installation
under `/private/tmp/wordcraft-npm-install` succeeded offline from the launcher and Darwin tarballs,
with both installed versions checked as `0.15.1`.

The committed Rust harness [`verify_delivery.rs`](../../crates/mcp/examples/verify_delivery.rs)
drove the same stdio workflow for all four runtime rows:

1. Initialize, verify the no-code workflow instructions and 16 compact tools.
2. Make an 18-phase proposal with one `doc_create` call; decode the resource and confirm its
   bytes exactly match the valid DOCX on disk. Check heading styles and keep-with-next.
3. Open the **saved DOCX in a fresh process**, inspect it and render all six pages.
4. Copy the synthetic DOCX to `uploads/synthetic-uuid/Original.docx`, start another process,
   and open that exact absolute upload path. Replace italic `careful` with `precise`, exercise
   command discovery/formatting, save as `Edited.docx`, and attach it.
5. Confirm the direct and meta attachment results are identical, the resource matches the saved
   file, the original upload bytes are unchanged, and a further fresh process reopens the edited
   file with the edit, unaffected title, styles and section properties preserved.

The same harness passed through the freshly installed npm launcher. Separate CLI integration
checks covered `WORDCRAFT_CLI`, `WORDCRAFT_ROOT`, cwd fallback, both explicit root spellings,
exit code 37 and SIGTERM forwarding (the child's trap returned code 23).

Example native reproduction (use a fresh scratch directory):

```sh
RUSTUP_TOOLCHAIN=1.97.1 cargo run -p wordcraft-mcp --example verify_delivery -- /private/tmp/wc-check /private/tmp/wc-check "$PWD/packaging/npm/dist/wordcraft-edge-mcp-darwin-arm64/bin/wordcraft-cli" mcp
```

For Alpine, pass the host scratch root, `/work`, and a command prefix such as
`docker run --rm -i --platform linux/arm64 -v HOST_ROOT:/work -v BINARY:/usr/local/bin/wordcraft-cli:ro alpine:3.22 wordcraft-cli mcp`.
The harness adds `--compact --root SERVER_ROOT` itself. Apply an outer timeout.

All three native-platform runs produced the same original DOCX SHA-256:
`75dba4f5dcd9fe79aa45222eb7651d3c3b464279a0fa1d0c3bd45ffd1fe306ca`.
Both created and edited documents rendered to six pages. Every Linux ARM64 page PNG matched
its x64 counterpart byte for byte. On each platform, pages 2–6 matched before and after the
single first-page text edit. macOS and Alpine use different available font fallbacks and their
PNG bytes differ; no identical cross-OS typography is claimed.

The native and Linux pages were visually inspected: headings, shaded/padded table headers,
row banding, repeated table headers, margins and pagination remained readable, with no clipped
text or broken table cells observed. Some tables/lists continue onto the next page normally.
Synthetic original/edit PNGs live under `/private/tmp/wordcraft-native-published`,
`/private/tmp/wordcraft-alpine-x64-final` and `/private/tmp/wordcraft-alpine-arm64-final`.
They are not committed. Native MCP runtime logs are `/tmp/wordcraft-native-runtime.log`,
`/tmp/wordcraft-alpine-x64-runtime.log`, `/tmp/wordcraft-alpine-arm64-runtime.log` and
`/tmp/wordcraft-npm-runtime.log`; staging/pack/install evidence is in
`/tmp/wordcraft-final-stage.log`, `/tmp/wordcraft-pack.jsonl` and `/tmp/wordcraft-npm-install.log`.

## Publication and live acceptance remain separate

No package was published and no LibreChat configuration, deployment or service was changed.
Read-only `npm view NAME version --prefer-online` checks for the launcher and all three platform
packages returned **E404**; no public version is verified. E404 can also reflect authentication
or approval problems and is not proof of publication success or package ownership.

Publish manually with the exact command in the [package guide](wordcraft-edge-mcp/README.md),
handle npm login/2FA, and verify every registry version afterward. Before connecting, create
`/app/uploads/wordcraft` on the uploads volume. A service force-update requires deployment
authorization, followed by installed-version and `wordcraft-cli tools` checks and two actual
LibreChat chat prompts (create and upload/edit/download).

The live model's tool choice, LibreChat upload copying, DOCX resource parser, actual deployment
image and chat download UX have **not** been accepted here. Arbitrary DOCX packages are not
claimed lossless: modeled formatting is preserved in these synthetic checks, while unsupported
OOXML can change or be lost. Detailed upstream importer warnings remain logs rather than an
exhaustive structured feature-loss report. Rooted output uses hard-link publication and retains
the existing parent-directory race limitation against a separate malicious local process.
Production needs per-user roots and filesystem isolation.
